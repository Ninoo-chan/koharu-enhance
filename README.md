<h1 align="center">Koharu Enhance</h1>

<p align="center">ML-powered manga translator, written in <b>Rust</b>.</p>

> [!IMPORTANT]
> **This repository is a fork of [Koharu](https://github.com/koharu-rs/koharu).**
> It contains personal enhancements and quality-of-life changes on top of the official project.
> For general information, installation instructions, documentation and guides, please read the
> **official Koharu README** at <https://github.com/koharu-rs/koharu>.

## Changes in this fork

Compared to the upstream [koharu-rs/koharu](https://github.com/koharu-rs/koharu), this fork adds:

### Text CSV import / export (menu "Text")
- New **Text** menu in the title bar with **Export text CSV** and **Import text CSV** actions.
- Export produces an RFC 4180-compliant CSV (with UTF-8 BOM) containing, for every text layer:
  page id / label, layer id, content id, original text + language, translation text + language, and role metadata.
- Import accepts the same structure: entries are matched by page/layer metadata and **overwrite** the existing
  text when the same text/metadata is found; new rows create new translations. Queries refresh automatically after import.
- Implemented in `crates/koharu-app/src/commands/text_csv.rs` with new Tauri commands registered in `commands/mod.rs`.

### Manual detection: Panel vs. Text
  - **Panel detection** (square icon): draws panel regions. Errors if a new panel overlaps an existing panel.
  - **Text detection** (original manual-detection icon): draws bubble/text regions. Requires an existing panel
    under the region (errors if you try to create text without a panel).
- Each tool only shows its own kind of region while active (panel view vs. bubbles/text view).
- **Double right-click deletes** the region under the cursor in both tools (deleting a panel also removes its
  child bubbles/text regions in cascade).
- Region hierarchy (`Inside` relations) is validated by geometry helpers in `packages/koharu/lib/geometry.ts`.

### Re-run OCR / Translation on emptied text
- **OCR**: running OCR again on a page re-processes it when the page has no text layers (e.g. after deleting them)
  or when any text layer's *original* text is empty. Layers whose original text was edited manually are preserved.
- **Translation**: running translation again re-translates any text layer whose *translation* is empty or missing
  (e.g. after clearing it by hand). Manually edited translations (`Origin::User`) are never overwritten.
- **Inpainting** skips pages that contain user-created text regions, avoiding cleanup over manual work.

### Pipeline & performance
- The scheduler now runs **one stage at a time across all pages** instead of pushing each page through the whole
  pipeline concurrently. This keeps at most one stage's model resident on the GPU, reducing VRAM pressure
  (`crates/koharu-pipeline/src/scheduler.rs`, `execution.rs`).
- Model unloading between stages via `StageProgress` signals (page/stage finished).

### Local LLM / VRAM fixes
- The multimodal projector is kept **on CPU** when the GPU has less than 8 GiB of VRAM, preventing OOM during
  prompt evaluation with Q4 models + BF16 vision projectors (`crates/koharu-translator/src/local/mod.rs`).
- New `use_gpu` option for the LLM projector (`crates/koharu-ml/src/llm`).

### Editor UI
- Font **weight (Bold), Italic and Underline** toggles in the text Inspector.
- The Inspector's top section is now scrollable and resizable-friendly (`max-h-[45%]`).
- Canvas state, job progress and page data refresh automatically as pipeline jobs advance (`providers.tsx`),
  fixing stale views after running stages.
- Manual text angle initialization fix (rotated regions keep their angle when created by hand).

---
