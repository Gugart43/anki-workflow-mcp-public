//! Deterministic validation and sanitization engine for `anki-workflow-mcp`.

pub mod cloze;
pub mod cloze_renumber;
pub mod crossrefs;
pub mod csv_structure;
pub mod hints;
pub mod id_leak;
pub mod images;
pub mod media_figure;
pub mod note_flow;
pub mod notes;
pub mod qa;
pub mod sanitize;
pub mod study_field;
pub mod table_render;
pub mod tables;
pub mod tags;
pub mod vocab;

pub use cloze::check_cloze;
pub use cloze_renumber::renumber_cloze;
pub use crossrefs::check_crossrefs;
pub use csv_structure::{check_bom, check_csv};
pub use hints::check_hints;
pub use id_leak::purge_id_leak;
pub use images::check_images_field;
pub use media_figure::{MediaFigure, render_media_figure};
pub use note_flow::{NoteFlow, render_note_flow};
pub use notes::{check_notes, check_notes_ext};
pub use qa::check_qa;
pub use sanitize::sanitize_field;
pub use study_field::infer_campo_estudo;
pub use table_render::{escape_cell, markdown_tables_to_html};
pub use tables::check_tables_field;
pub use tags::check_tags;
