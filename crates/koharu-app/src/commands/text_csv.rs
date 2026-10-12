use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use koharu_desktop::Desktop;
use koharu_scene::{
    Authored, EntityId, LanguageTag, Origin, Snapshot, SourceText as SceneSourceText, TextRole,
    Translation as SceneTranslation,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{State, WebviewWindow};
use tauri_runtime_cef::CefRuntime;

use super::{
    ChannelExt as _, Error,
    canvas::CanvasChannel,
    editing::synchronize_canvas,
    project::{CurrentProject, Project},
};

const CSV_HEADER: &str = concat!(
    "page_id,page_label,layer_id,content_id,role,",
    "original_text,original_language,translation_text,translation_language\n"
);

/// A single text row of the project, paired with its translation and metadata.
#[derive(Clone, Debug, Serialize, Type)]
pub struct TextRow {
    pub page_id: EntityId,
    pub page_label: String,
    pub layer_id: EntityId,
    pub content_id: EntityId,
    pub role: Option<String>,
    pub original_text: Option<String>,
    pub original_language: Option<String>,
    pub translation_text: Option<String>,
    pub translation_language: Option<String>,
}

/// One parsed CSV record destined for a specific text layer.
#[derive(Clone, Debug, Deserialize, Type)]
pub struct TextCsvEntry {
    pub layer: EntityId,
    pub original_text: Option<String>,
    pub original_language: Option<String>,
    pub translation_text: Option<String>,
    pub translation_language: Option<String>,
    pub role: Option<String>,
}

/// Summary of a completed CSV import.
#[derive(Clone, Copy, Debug, Default, Serialize, Type)]
pub struct TextCsvImportSummary {
    pub updated: u32,
    pub skipped: u32,
}

fn csv_field(value: Option<&str>) -> String {
    let value = value.unwrap_or_default();
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                current.push('"');
                characters.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    fields.push(current);
    fields
}

fn parse_optional(fields: &[String], index: usize) -> Option<String> {
    fields
        .get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Serializes rows into a UTF-8 (BOM-prefixed) RFC 4180 CSV document.
#[must_use]
pub fn encode_text_csv(rows: &[TextRow]) -> Vec<u8> {
    let mut document = String::from(CSV_HEADER);
    for row in rows {
        let fields = [
            Some(row.page_id.to_string().as_str()),
            Some(row.page_label.as_str()),
            Some(row.layer_id.to_string().as_str()),
            Some(row.content_id.to_string().as_str()),
            row.role.as_deref(),
            row.original_text.as_deref(),
            row.original_language.as_deref(),
            row.translation_text.as_deref(),
            row.translation_language.as_deref(),
        ];
        document.push_str(
            &fields
                .into_iter()
                .map(csv_field)
                .collect::<Vec<_>>()
                .join(","),
        );
        document.push('\n');
    }
    let mut bytes = Vec::with_capacity(document.len() + 3);
    bytes.extend_from_slice(b"\xEF\xBB\xBF");
    bytes.extend_from_slice(document.as_bytes());
    bytes
}

/// Parses a CSV document produced by [`encode_text_csv`] back into entries.
///
/// Rows are keyed by `layer_id`; rows referencing unknown layers or empty
/// layer ids are skipped so that stale exports never clobber unrelated data.
#[must_use]
pub fn decode_text_csv(bytes: &[u8], snapshot: &Snapshot) -> Result<Vec<TextCsvEntry>> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let mut pages_by_label: HashMap<&str, Vec<EntityId>> = HashMap::new();
    let mut layers_by_page: HashMap<EntityId, Vec<(EntityId, Option<String>)>> = HashMap::new();
    for page in snapshot.pages() {
        let id = page.id();
        let label = page.page()?.label;
        pages_by_label
            .entry(label.as_str())
            .or_default()
            .push(id);
        let mut layers = Vec::new();
        if let Some(group) = page.text_group()? {
            for layer in group.text_layers()? {
                let source = layer
                    .content()?
                    .source()?
                    .map(|source| source.text.value);
                layers.push((layer.id(), source));
            }
        }
        layers_by_page.insert(id, layers);
    }

    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let fields = split_csv_line(trimmed);
        let first = fields.first().map(String::as_str).unwrap_or_default();
        if first.eq_ignore_ascii_case("page_id") || first.eq_ignore_ascii_case("page") {
            continue;
        }
        let Some(layer_field) = parse_optional(&fields, 2) else {
            continue;
        };
        let layer: EntityId = uuid::Uuid::parse_str(&layer_field)
            .map_err(|error| anyhow::anyhow!("invalid layer id {layer_field}: {error}"))?
            .into();
        if snapshot.text_layer(layer).is_err() {
            continue;
        }
        entries.push(TextCsvEntry {
            layer,
            original_text: parse_optional(&fields, 5),
            original_language: parse_optional(&fields, 6),
            translation_text: parse_optional(&fields, 7),
            translation_language: parse_optional(&fields, 8),
            role: parse_optional(&fields, 4),
        });
    }
    Ok(entries)
}

fn collect_rows(snapshot: &Snapshot) -> Result<Vec<TextRow>> {
    let mut rows = Vec::new();
    for page in snapshot.pages() {
        let page_id = page.id();
        let page_label = page.page()?.label;
        let Some(group) = page.text_group()? else {
            continue;
        };
        for layer in group.text_layers()? {
            let content = layer.content()?;
            let source = content.source()?;
            let translation = content.translation()?;
            let role = content.role()?;
            rows.push(TextRow {
                page_id,
                page_label: page_label.clone(),
                layer_id: layer.id(),
                content_id: content.id(),
                role: role.map(|role| role.role),
                original_text: source.clone().map(|source| source.text.value),
                original_language: source
                    .and_then(|source| source.language)
                    .map(|language| language.to_string()),
                translation_text: translation.clone().map(|translation| translation.text.value),
                translation_language: translation
                    .and_then(|translation| translation.language)
                    .map(|language| language.to_string()),
            });
        }
    }
    Ok(rows)
}

async fn apply_entry(project: &mut Project, entry: TextCsvEntry) -> Result<Option<koharu_scene::Commit>> {
    let snapshot = project.snapshot();
    let layer = snapshot.text_layer(entry.layer)?;
    let content = layer.content()?;
    let content_id = content.id();

    let current_source = content.source()?;
    let current_translation = content.translation()?;
    let current_role = content.role()?;

    let next_original = entry.original_text.clone();
    let next_translation = entry.translation_text.clone();
    let next_original_language = entry.original_language.clone();
    let next_translation_language = entry.translation_language.clone();
    let next_role = entry.role.clone();

    let source_unchanged = current_source
        .as_ref()
        .map(|source| source.text.value.clone())
        == next_original;
    let source_language_unchanged = current_source
        .as_ref()
        .and_then(|source| source.language.as_ref())
        .map(ToString::to_string)
        == next_original_language.clone();
    let translation_unchanged = current_translation
        .as_ref()
        .map(|translation| translation.text.value.clone())
        == next_translation;
    let translation_language_unchanged = current_translation
        .as_ref()
        .and_then(|translation| translation.language.as_ref())
        .map(ToString::to_string)
        == next_translation_language.clone();
    let role_unchanged = current_role
        .as_ref()
        .map(|role| role.role.clone())
        == next_role.clone();

    if source_unchanged
        && source_language_unchanged
        && translation_unchanged
        && translation_language_unchanged
        && role_unchanged
    {
        return Ok(None);
    }

    let patch = snapshot.patch(|edit| {
        edit.promote_entity_to_user(entry.layer)?;
        edit.promote_entity_to_user(content_id)?;
        if let Some(text) = next_original.clone() {
            let language = match next_original_language.clone() {
                Some(tag) => Some(LanguageTag::new(tag)?),
                None => None,
            };
            edit.set(
                content_id,
                &SceneSourceText {
                    text: Authored::user(text),
                    language,
                },
            )?;
        }
        match next_translation.clone() {
            Some(text) => {
                let language = match next_translation_language.clone() {
                    Some(tag) => Some(LanguageTag::new(tag)?),
                    None => None,
                };
                edit.set(
                    content_id,
                    &SceneTranslation {
                        text: Authored::user(text),
                        language,
                    },
                )?;
            }
            None => {
                if next_original.is_some() {
                    edit.remove::<SceneTranslation>(content_id)?;
                }
            }
        }
        match next_role.clone() {
            Some(role) => edit.set(
                content_id,
                &TextRole {
                    origin: Origin::User,
                    role,
                },
            )?,
            None => {
                if current_role.is_some() {
                    edit.remove::<TextRole>(content_id)?;
                }
            }
        }
        Ok(())
    })?;
    Ok(Some(project.commit_rebased(patch).await?))
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "text_csv_exported",
    skip_all,
    fields(origin = "user"),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn export_text_csv(
    window: WebviewWindow<CefRuntime>,
    project: State<'_, CurrentProject>,
) -> Result<(), Error> {
    let (name, snapshot) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        (project.name.clone(), project.snapshot())
    };
    let rows = collect_rows(&snapshot)?;
    if rows.is_empty() {
        bail!("there are no text layers to export");
    }
    let csv = encode_text_csv(&rows);
    let destination = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("Comma Separated Values", &["csv"])
        .set_file_name(format!("{name}.csv"))
        .save_file()
        .await;
    let Some(destination) = destination else {
        return Ok(());
    };
    tokio_rayon::spawn(move || -> Result<()> {
        std::fs::write(destination.path(), csv)?;
        Ok(())
    })
    .await?;
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "text_csv_imported",
    skip_all,
    fields(origin = "user"),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn import_text_csv(
    window: WebviewWindow<CefRuntime>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<TextCsvImportSummary, Error> {
    let file = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("Comma Separated Values", &["csv"])
        .pick_file()
        .await;
    let Some(file) = file else {
        return Ok(TextCsvImportSummary::default());
    };
    let bytes = tokio_rayon::spawn(move || std::fs::read(file.path())).await?;
    let mut updated = 0_u32;
    let mut skipped = 0_u32;
    let commits = {
        let mut guard = project.project.lock().await;
        let current = guard.as_mut().context("no project is open")?;
        let snapshot = current.snapshot();
        let entries = decode_text_csv(&bytes, &snapshot)?;
        let mut commits = Vec::with_capacity(entries.len());
        for entry in entries {
            match apply_entry(current, entry).await? {
                Some(commit) => {
                    current.record_commit(&commit);
                    commits.push(commit);
                    updated += 1;
                }
                None => skipped += 1,
            }
        }
        (commits, current.active_page())
    };
    let (commits, active_page) = commits;
    for commit in &commits {
        let canvas = synchronize_canvas(&desktop, commit, active_page).await?;
        canvas_channel.channel.publish(canvas);
    }
    Ok(TextCsvImportSummary { updated, skipped })
}
