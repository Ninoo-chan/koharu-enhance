use anyhow::{Context as _, bail};
use image::{RgbaImage, imageops::FilterType};
use koharu_desktop::{CanvasState, Desktop};
use koharu_scene::EntityId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{State, WebviewWindow};
use tauri_runtime_cef::CefRuntime;

use super::{
    ChannelExt as _, Error,
    canvas::{CanvasChannel, LayerCommit, Point},
    project::{CurrentProject, Page, Project, Typography},
};
pub(crate) async fn synchronize_canvas(
    desktop: &Desktop,
    commit: &koharu_scene::Commit,
    page: Option<EntityId>,
) -> anyhow::Result<CanvasState> {
    desktop.synchronize(&commit.snapshot, page, commit).await?;
    Ok(desktop.canvas_state())
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct GeometryUpdate {
    pub layer: EntityId,
    pub points: Option<Vec<Point>>,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct TypographyUpdate {
    pub layer: EntityId,
    pub typography: Typography,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
pub struct DetectionRegionCommit {
    pub revision: koharu_scene::Revision,
    pub region: EntityId,
    pub layer: Option<EntityId>,
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "page_renamed",
    skip_all,
    fields(origin = "user", character_count = label.chars().count(), empty = label.is_empty()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_page(
    page: EntityId,
    label: String,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.rename_page(page, label).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "pages_deleted",
    skip_all,
    fields(origin = "user", entity_count = pages.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_pages(
    pages: Vec<EntityId>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.delete_pages(pages).await?;
        project.record_commit(&commit);
        project.reconcile_page();
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "page_moved",
    skip_all,
    fields(origin = "user", page_number = index + 1),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn move_page(
    page: EntityId,
    index: u32,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.move_page(page, index as usize).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "source_text_edited",
    skip_all,
    fields(origin = "user", character_count = text.chars().count(), empty = text.is_empty()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_source_text(
    layer: EntityId,
    text: String,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_source_text(layer, text).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "translation_edited",
    skip_all,
    fields(
        origin = "user",
        character_count = text.as_ref().map_or(0, |text| text.chars().count()),
        empty = text.as_ref().is_none_or(String::is_empty),
    ),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_translation(
    layer: EntityId,
    text: Option<String>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_translation(layer, text).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "typography_edited",
    skip_all,
    fields(origin = "user", entity_count = updates.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_typography(
    updates: Vec<TypographyUpdate>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_typography(updates).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "geometry_edited",
    skip_all,
    fields(origin = "user", entity_count = updates.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_geometry(
    updates: Vec<GeometryUpdate>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_geometry(updates).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "detection_region_added",
    skip_all,
    fields(origin = "user", point_count = points.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn add_manual_detection_region(
    points: Vec<Point>,
    panel: bool,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<DetectionRegionCommit, Error> {
    let (commit, page, region, layer) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let page = project.active_page().context("the project has no active page")?;
        let (commit, region, layer) = if panel {
            let (commit, region) = project.add_manual_panel_region(page, points).await?;
            (commit, region, None)
        } else {
            let (commit, region, layer) = project.add_manual_text_region(page, points).await?;
            (commit, region, Some(layer))
        };
        project.record_commit(&commit);
        (commit, page, region, layer)
    };
    let canvas = synchronize_canvas(&desktop, &commit, Some(page)).await?;
    canvas_channel.channel.publish(canvas);
    Ok(DetectionRegionCommit {
        revision: commit.revision,
        region,
        layer,
    })
}

/// Name of the layer created from a picked picture: the cleaned version of the page illustration.
const IMAGE_LAYER_NAME: &str = "Cleaned illustration";

/// Scales `image` to exactly the page size. A picture whose proportions differ from the page's
/// is rejected instead of being stretched, so a mismatched file never distorts silently.
fn fit_to_page(image: RgbaImage, width: u32, height: u32) -> anyhow::Result<RgbaImage> {
    let (source_width, source_height) = image.dimensions();
    if width == 0 || height == 0 || source_width == 0 || source_height == 0 {
        bail!("the page and the image must have positive dimensions");
    }
    if (source_width, source_height) == (width, height) {
        return Ok(image);
    }
    let page_ratio = f64::from(width) / f64::from(height);
    let image_ratio = f64::from(source_width) / f64::from(source_height);
    if (image_ratio / page_ratio - 1.0).abs() > 0.01 {
        bail!(
            "the image is {source_width}x{source_height} but the page is {width}x{height}; \
             its proportions must match the page"
        );
    }
    Ok(image::imageops::resize(
        &image,
        width,
        height,
        FilterType::Lanczos3,
    ))
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "image_layer_added",
    skip_all,
    fields(origin = "user")
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn add_image_layer(
    window: WebviewWindow<CefRuntime>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<Option<LayerCommit>, Error> {
    let (page, width, height) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        let page = project
            .active_page()
            .context("the project has no active page")?;
        let size = project.snapshot().page(page)?.page()?;
        (page, size.width.round() as u32, size.height.round() as u32)
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
        .set_parent(&window)
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    let image = tokio_rayon::spawn(move || {
        let image = image::open(&path)
            .with_context(|| format!("failed to read {}", path.display()))?
            .to_rgba8();
        fit_to_page(image, width, height)
    })
    .await?;
    let (commit, layer, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let (commit, layer) = project
            .add_image_layer(page, IMAGE_LAYER_NAME, image)
            .await?;
        project.record_commit(&commit);
        (commit, layer, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(Some(LayerCommit {
        revision: commit.revision,
        layer,
    }))
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "detection_regions_deleted",
    skip_all,
    fields(origin = "user", entity_count = regions.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_detection_regions(
    regions: Vec<EntityId>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.delete_detection_regions(regions).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "visibility_edited",
    skip_all,
    fields(origin = "user", entity_count = layers.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_visibility(
    layers: Vec<EntityId>,
    visible: Option<bool>,
    opacity: Option<f32>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.set_visibility(layers, visible, opacity).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "layers_deleted",
    skip_all,
    fields(origin = "user", entity_count = layers.len()),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_layers(
    layers: Vec<EntityId>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.delete_layers(layers).await?;
        project.record_commit(&commit);
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "layer_moved",
    skip_all,
    fields(origin = "user", entity_count = 1_u64)
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn move_layer(
    layer: EntityId,
    parent: EntityId,
    index: u32,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<Page, Error> {
    let (commit, page, view) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.move_layer(layer, parent, index as usize).await?;
        project.record_commit(&commit);
        let page = project.active_page().context("no active page")?;
        let view = Project::page(&commit.snapshot, page)?;
        (commit, page, view)
    };
    desktop
        .synchronize(&commit.snapshot, Some(page), &commit)
        .await?;
    canvas_channel.channel.publish(desktop.canvas_state());
    Ok(view)
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "undo",
    skip_all,
    fields(origin = "user")
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn undo(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.undo().await?;
        project.reconcile_page();
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "redo",
    skip_all,
    fields(origin = "user")
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn redo(
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_channel: State<'_, CanvasChannel>,
) -> Result<(), Error> {
    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let commit = project.redo().await?;
        project.reconcile_page();
        (commit, project.active_page())
    };
    let canvas = synchronize_canvas(&desktop, &commit, page).await?;
    canvas_channel.channel.publish(canvas);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_image_is_kept_as_is() {
        let image = RgbaImage::from_pixel(40, 60, image::Rgba([1, 2, 3, 255]));
        let fitted = fit_to_page(image.clone(), 40, 60).unwrap();
        assert_eq!(fitted, image);
    }

    #[test]
    fn image_with_page_proportions_is_scaled_to_the_page() {
        let image = RgbaImage::from_pixel(20, 30, image::Rgba([9, 9, 9, 255]));
        let fitted = fit_to_page(image, 40, 60).unwrap();
        assert_eq!(fitted.dimensions(), (40, 60));
    }

    #[test]
    fn image_with_other_proportions_is_rejected() {
        let image = RgbaImage::new(60, 60);
        let error = fit_to_page(image, 40, 60).unwrap_err().to_string();
        assert!(error.contains("proportions"), "{error}");
    }

    #[test]
    fn empty_dimensions_are_rejected() {
        assert!(fit_to_page(RgbaImage::new(10, 10), 0, 10).is_err());
    }
}
