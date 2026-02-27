use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
    mtmd::{MtmdContext, MtmdContextParams},
};
use std::ffi::CString;
use std::io::{Read, Seek, SeekFrom};

/// Redirect C-level stdout to stderr using _dup2
fn fix_c_stdout() {
    #[cfg(windows)]
    {
        extern "C" {
            fn _dup2(fd1: i32, fd2: i32) -> i32;
        }
        unsafe {
            let ret = _dup2(2, 1);
            if ret == -1 {
                eprintln!("Warning: _dup2(2, 1) failed");
            } else {
                eprintln!("C stdout (fd 1) redirected to stderr (fd 2)");
            }
        }
    }
}

/// Test that we can seekg on the mmproj file from Rust before llama.cpp touches it
fn test_file_seek(path: &str) {
    eprintln!("=== Pre-test: testing file seek from Rust ===");
    let mut f = std::fs::File::open(path).expect("failed to open mmproj");
    let metadata = f.metadata().expect("metadata");
    let file_size = metadata.len();
    eprintln!("  File size: {} bytes", file_size);

    // Seek to the same offset that clip.cpp will try: 854195616
    let offset = 854195616u64;
    eprintln!("  Seeking to offset {}...", offset);
    f.seek(SeekFrom::Start(offset)).expect("seek failed");
    eprintln!("  Seek OK! Reading 4608 bytes...");
    let mut buf = vec![0u8; 4608];
    f.read_exact(&mut buf).expect("read failed");
    eprintln!("  Read OK! First 16 bytes: {:?}", &buf[..16]);
    eprintln!("=== Pre-test PASSED ===");
}

fn main() {
    fix_c_stdout();

    let model_path = r"E:/.lmstudio/lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q8_0.gguf";
    let mmproj_path = r"E:/.lmstudio/lmstudio-community/gemma-3-12b-it-GGUF/mmproj-model-f16.gguf";

    // Test that file I/O works before loading anything
    test_file_seek(mmproj_path);

    eprintln!("Initializing backend...");
    let backend = LlamaBackend::init().expect("backend init");

    eprintln!("Loading model from {}...", model_path);
    let model_params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .expect("model load");

    // Test again AFTER model loading to see if model loading corrupts CRT state
    eprintln!("=== Post-model-load seek test ===");
    test_file_seek(mmproj_path);

    eprintln!("Model loaded. Loading mmproj from {}...", mmproj_path);
    let params = MtmdContextParams {
        use_gpu: false,
        print_timings: false,
        n_threads: 4,
        media_marker: CString::new("<__media__>").unwrap(),
    };

    match MtmdContext::init_from_file(mmproj_path, &model, &params) {
        Ok(ctx) => {
            eprintln!("Vision context initialized! vision={}", ctx.support_vision());
        }
        Err(e) => {
            eprintln!("ERROR: Failed to init vision context: {}", e);
        }
    }
}
