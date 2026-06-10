//! Office document format extractors: DOCX, PPTX, XLSX, ODT.
use std::io::Read;

/// Extract plain text from DOCX bytes (ZIP containing word/document.xml with <w:t> tags).
pub fn extract_docx_text(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading DOCX archive: {e}"),
    };

    let mut xml_content = String::new();
    match archive.by_name("word/document.xml") {
        Ok(mut entry) => {
            if let Err(e) = entry.read_to_string(&mut xml_content) {
                return format!("Error reading DOCX document.xml: {e}");
            }
        }
        Err(e) => return format!("Error: not a valid DOCX file (missing word/document.xml): {e}"),
    }

    let mut reader = quick_xml::Reader::from_str(&xml_content);
    let mut text = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"p" && !text.is_empty() {
                    text.push('\n');
                } else if local.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.local_name().as_ref() == b"t" {
                    in_t = false;
                }
            }
            Ok(quick_xml::events::Event::Text(e)) if in_t => {
                if let Ok(s) = e.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return format!("Error parsing DOCX XML: {e}"),
            _ => {}
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(DOCX contains no extractable text)".to_string();
    }

    eprintln!("[READ_FILE/DOCX] Extracted {} chars from DOCX", text.len());
    if text.len() > max_chars {
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        format!("{}\n\n[DOCX truncated at {} chars]", &text[..end], max_chars)
    } else {
        text
    }
}

/// Extract plain text from PPTX bytes (ZIP containing ppt/slide*.xml with <a:t> tags).
pub fn extract_pptx_text(bytes: &[u8], max_chars: usize) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading PPTX archive: {e}"),
    };

    // Collect slide file names and sort them (slide1.xml, slide2.xml, ...)
    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .collect();
    slide_names.sort();

    if slide_names.is_empty() {
        return "(PPTX contains no slides)".to_string();
    }

    let mut text = String::new();
    for (idx, slide_name) in slide_names.iter().enumerate() {
        let mut xml_content = String::new();
        match archive.by_name(slide_name) {
            Ok(mut entry) => {
                if entry.read_to_string(&mut xml_content).is_err() {
                    continue;
                }
            }
            Err(_) => continue,
        }

        if !text.is_empty() { text.push('\n'); }
        text.push_str(&format!("--- Slide {} ---\n", idx + 1));

        // Extract text from <a:t> tags (DrawingML text elements)
        let mut reader = quick_xml::Reader::from_str(&xml_content);
        let mut in_t = false;
        let mut slide_has_text = false;

        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
                Ok(quick_xml::events::Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
                Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"p" => {
                    if slide_has_text { text.push('\n'); }
                }
                Ok(quick_xml::events::Event::Text(e)) if in_t => {
                    if let Ok(s) = e.unescape() {
                        text.push_str(&s);
                        slide_has_text = true;
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(PPTX contains no extractable text)".to_string();
    }

    eprintln!("[READ_FILE/PPTX] Extracted {} chars from {} slides", text.len(), slide_names.len());
    crate::truncate_text_content(&text, max_chars)
}

/// Extract spreadsheet data as tab-separated text from XLSX/XLS/XLSM bytes.
pub fn extract_xlsx_text(bytes: &[u8], max_chars: usize) -> String {
    use calamine::{Reader, Data};

    let cursor = std::io::Cursor::new(bytes);
    let mut workbook = match calamine::open_workbook_auto_from_rs(cursor) {
        Ok(wb) => wb,
        Err(e) => return format!("Error reading spreadsheet: {e}"),
    };

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return "(Spreadsheet contains no sheets)".to_string();
    }

    let mut text = String::new();
    for sheet_name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            if !text.is_empty() { text.push('\n'); }
            text.push_str(&format!("=== {} ===\n", sheet_name));

            for row in range.rows() {
                let row_values: Vec<String> = row.iter().map(|cell| match cell {
                    Data::String(s) => s.clone(),
                    Data::Int(i) => i.to_string(),
                    Data::Float(f) => {
                        // Show integers without decimal point
                        if *f == f.trunc() && f.abs() < 1e15 { format!("{}", *f as i64) } else { f.to_string() }
                    }
                    Data::Bool(b) => b.to_string(),
                    Data::DateTime(dt) => format!("{dt}"),
                    Data::DateTimeIso(s) => s.clone(),
                    Data::DurationIso(s) => s.clone(),
                    Data::Error(e) => format!("ERR:{e:?}"),
                    Data::Empty => String::new(),
                }).collect();
                text.push_str(&row_values.join("\t"));
                text.push('\n');
            }
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(Spreadsheet contains no data)".to_string();
    }

    eprintln!("[READ_FILE/XLSX] Extracted {} chars from {} sheets", text.len(), sheet_names.len());
    crate::truncate_text_content(&text, max_chars)
}

/// Extract plain text from ODT bytes (OpenDocument Text: ZIP with content.xml containing <text:p> tags).
pub fn extract_odt_text(bytes: &[u8], max_chars: usize) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use std::io::Read;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => return format!("Error reading ODT archive: {e}"),
    };

    let mut xml_content = String::new();
    match archive.by_name("content.xml") {
        Ok(mut entry) => {
            if let Err(e) = entry.read_to_string(&mut xml_content) {
                return format!("Error reading content.xml: {e}");
            }
        }
        Err(e) => return format!("Error finding content.xml in ODT: {e}"),
    }

    let mut reader = Reader::from_str(&xml_content);
    let mut text = String::new();
    let mut in_text_element = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                // <text:p>, <text:h>, <text:span> contain text
                if name == "p" || name == "h" || name == "span" {
                    in_text_element = true;
                    if (name == "p" || name == "h") && !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                }
                // <text:tab/> → tab, <text:line-break/> → newline
                if name == "tab" { text.push('\t'); }
                if name == "line-break" { text.push('\n'); }
            }
            Ok(Event::Text(ref e)) if in_text_element => {
                if let Ok(t) = e.unescape() {
                    text.push_str(&t);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                if name == "p" || name == "h" || name == "span" {
                    in_text_element = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!("[READ_FILE/ODT] XML parse error: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return "(ODT document contains no text)".to_string();
    }

    eprintln!("[READ_FILE/ODT] Extracted {} chars", text.len());
    crate::truncate_text_content(&text, max_chars)
}
