//! Deterministic validation and sanitization engine for `anki-workflow-mcp`.

pub mod batch_information;
pub mod cloze;
pub mod cloze_renumber;
pub mod crossrefs;
pub mod csv_structure;
pub mod domain_profile;
pub mod draft_compile;
pub mod draft_normalize;
pub mod draft_patch;
pub mod hints;
mod html_security;
pub mod html_text;
pub mod id_leak;
pub mod image_alias;
pub mod images;
pub mod legacy_adapter;
pub mod media_figure;
pub mod note_flow;
pub mod note_metrics;
pub mod note_model;
pub mod note_render;
pub mod notes;
pub mod notes_render;
pub mod qa;
pub mod quality;
pub mod quality_content;
pub mod sanitize;
pub mod study_field;
pub mod table_render;
pub mod tables;
pub mod tags;
pub mod validated_flow;
pub mod vocab;

pub use batch_information::{
    BatchInformationReview, CardInformationReview, review_batch_information,
};
pub use cloze::check_cloze;
pub use cloze_renumber::renumber_cloze;
pub use crossrefs::check_crossrefs;
pub use csv_structure::{check_bom, check_csv};
pub use domain_profile::{DomainProfile, DomainProfileError};
pub use draft_compile::{compile_drafts, compile_drafts_strict};
pub use draft_normalize::{
    CardTipo, RawCardDraft, StrictRawCardDraft, normalize_draft, normalize_draft_strict,
};
pub use draft_patch::{partition_quarantine, patch_draft};
pub use hints::check_hints;
pub use html_text::{
    html_to_visible_text, normalize_for_exact_match, parse_html_text, visible_char_count,
};
pub use id_leak::purge_id_leak;
pub use images::check_images_field;
pub use legacy_adapter::{
    AdapterError, CompileNotesError, CompiledNotes, DraftNotesInput, compile_extra_notes,
    normalize_to_document,
};
pub use media_figure::{MediaFigure, render_media_figure};
pub use note_flow::{NoteFlow, render_note_flow};
pub use note_metrics::{
    NotesStructureMetrics, check_cloze_target_restatement, compute_notes_metrics,
};
pub use note_model::{
    CalloutKind, CompileMode, KeyValueItem, NoteBlock, NoteDocument, NoteSource, SourceLocation,
};
pub use note_render::{RenderError, RenderedNotes, render_document};
pub use notes::{check_notes, check_notes_ext};
pub use notes_render::render_extra_notes;
pub use qa::check_qa;
pub use quality::{
    QUALITY_SCHEMA_VERSION, QualityBinding, QualityDimensions, QualityEvidence, QualityInput,
    QualityMode, QualityObligation, QualityReport, QualityReview, QualitySource, evaluate_quality,
    prepare_quality,
};
pub use quality::{QualityPrepareInput, prepare_quality_input};
pub use quality_content::review_content;
pub use sanitize::sanitize_field;
pub use study_field::infer_campo_estudo;
pub use table_render::{escape_cell, markdown_tables_to_html};
pub use tables::check_tables_field;
pub use tags::check_tags;
pub use validated_flow::{FlowError, FlowTopology, RawFlow, ValidatedFlow, render_validated_flow};
