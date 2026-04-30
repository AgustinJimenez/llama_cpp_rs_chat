//! Compare PDF text extraction crates

fn main() {
    let pdf_path = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/repo/tmp_project/nemesis.pdf".to_string()
    });
    println!("Testing PDF: {}\n", pdf_path);

    // Test 1: pdf-extract (current)
    println!("=== pdf-extract (current) ===");
    let start = std::time::Instant::now();
    match pdf_extract::extract_text(&pdf_path) {
        Ok(text) => {
            println!("Time: {}ms", start.elapsed().as_millis());
            println!("Text length: {} chars", text.len());
            let trimmed = text.trim();
            if trimmed.is_empty() {
                println!("Result: EMPTY");
            } else {
                println!("Preview: {:?}", &trimmed[..trimmed.len().min(300)]);
            }
        }
        Err(e) => println!("Error: {e}"),
    }

    // Test 2: pdf_oxide
    println!("\n=== pdf_oxide ===");
    let start = std::time::Instant::now();
    match pdf_oxide::PdfDocument::open(&pdf_path) {
        Ok(mut doc) => {
            let page_count = doc.page_count().unwrap_or(0);
            let mut total_text = String::new();
            for i in 0..page_count.min(10) {
                match doc.extract_text(i) {
                    Ok(text) => total_text.push_str(&text),
                    Err(e) => eprintln!("  Page {} error: {}", i, e),
                }
            }
            println!("Time: {}ms", start.elapsed().as_millis());
            println!("Pages: {}", page_count);
            println!("First 10 pages: {} chars", total_text.len());
            let trimmed = total_text.trim();
            if trimmed.is_empty() {
                println!("Result: EMPTY");
            } else {
                println!("Preview: {:?}", &trimmed[..trimmed.len().min(300)]);
            }
        }
        Err(e) => println!("Error: {e}"),
    }
}
