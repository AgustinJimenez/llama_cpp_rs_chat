//! Document format extractors.

mod html_markdown;
mod pdf;
mod office;
mod misc;

pub use pdf::{extract_pdf_text, extract_pdf_with_pages};
pub use office::{extract_docx_text, extract_pptx_text, extract_xlsx_text, extract_odt_text};
pub use misc::{extract_epub_text, extract_rtf_text, extract_zip_listing, extract_csv_structured, extract_eml_text};
pub(crate) use html_markdown::{convert_and_truncate, extract_body_content};
