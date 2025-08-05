use crate::ai_operations::*;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ProjectTemplateManager {
    templates: HashMap<String, ProjectTemplate>,
}

impl ProjectTemplateManager {
    pub fn new() -> Self {
        let mut templates = HashMap::new();
        
        // Add built-in templates
        templates.insert("rust-cli".to_string(), Self::create_rust_cli_template());
        templates.insert("rust-lib".to_string(), Self::create_rust_lib_template());
        templates.insert("python-cli".to_string(), Self::create_python_cli_template());
        templates.insert("node-app".to_string(), Self::create_node_app_template());
        templates.insert("web-app".to_string(), Self::create_web_app_template());
        templates.insert("config-files".to_string(), Self::create_config_files_template());
        
        Self { templates }
    }

    fn create_rust_cli_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "rust-cli".to_string(),
            description: "Rust command-line application with CLI argument parsing".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from("Cargo.toml"),
                    content: r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
clap = { version = "4.5", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("src/main.rs"),
                    content: r#"use anyhow::Result;
use clap::{Arg, Command, Parser};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file
    #[arg(short, long)]
    input: Option<String>,
    
    /// Output file  
    #[arg(short, long)]
    output: Option<String>,
    
    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    if args.verbose {
        println!("Running {{project_name}} with verbose output");
    }
    
    match args.input {
        Some(input) => println!("Input file: {}", input),
        None => println!("No input file specified"),
    }
    
    match args.output {
        Some(output) => println!("Output file: {}", output),
        None => println!("No output file specified"),
    }
    
    println!("Hello, world from {{project_name}}!");
    
    Ok(())
}
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("README.md"),
                    content: r#"# {{project_name}}

A Rust command-line application.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
{{project_name}} --help
{{project_name}} --input input.txt --output output.txt --verbose
```

## Development

```bash
# Run the application
cargo run

# Run tests
cargo test

# Build release version
cargo build --release
```
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from(".gitignore"),
                    content: r#"/target/
**/*.rs.bk
Cargo.lock
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![
                "cargo check".to_string(),
                "cargo test".to_string(),
            ],
            dependencies: vec!["rust".to_string(), "cargo".to_string()],
        }
    }

    fn create_rust_lib_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "rust-lib".to_string(),
            description: "Rust library crate with documentation and tests".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from("Cargo.toml"),
                    content: r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"
authors = ["Your Name <your.email@example.com>"]
description = "A Rust library"
license = "MIT OR Apache-2.0"
repository = "https://github.com/yourusername/{{project_name}}"
keywords = ["rust", "library"]
categories = ["development-tools"]

[dependencies]

[dev-dependencies]
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("src/lib.rs"),
                    content: r#"//! # {{project_name}}
//!
//! A Rust library for...

/// Main functionality of the library
pub fn main_function() -> String {
    "Hello from {{project_name}}!".to_string()
}

/// A helper function
pub fn helper_function(input: &str) -> String {
    format!("Processed: {}", input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_function() {
        let result = main_function();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_helper_function() {
        let result = helper_function("test");
        assert_eq!(result, "Processed: test");
    }
}
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("README.md"),
                    content: r#"# {{project_name}}

A Rust library for...

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
{{project_name}} = "0.1.0"
```

## Usage

```rust
use {{project_name}}::main_function;

fn main() {
    let result = main_function();
    println!("{}", result);
}
```

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![
                "cargo check".to_string(),
                "cargo test".to_string(),
                "cargo doc".to_string(),
            ],
            dependencies: vec!["rust".to_string(), "cargo".to_string()],
        }
    }

    fn create_python_cli_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "python-cli".to_string(),
            description: "Python command-line application with argument parsing".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from("main.py"),
                    content: r#"#!/usr/bin/env python3
"""
{{project_name}} - A Python CLI application
"""

import argparse
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description='{{project_name}} CLI application')
    parser.add_argument('--input', '-i', type=str, help='Input file')
    parser.add_argument('--output', '-o', type=str, help='Output file')
    parser.add_argument('--verbose', '-v', action='store_true', help='Verbose output')
    
    args = parser.parse_args()
    
    if args.verbose:
        print(f"Running {{project_name}} with verbose output")
    
    if args.input:
        print(f"Input file: {args.input}")
        if not Path(args.input).exists():
            print(f"Warning: Input file {args.input} does not exist")
    
    if args.output:
        print(f"Output file: {args.output}")
    
    print("Hello, world from {{project_name}}!")


if __name__ == "__main__":
    main()
"#.to_string(),
                    permissions: Some(0o755),
                    is_executable: true,
                },
                FileTemplate {
                    path: PathBuf::from("requirements.txt"),
                    content: r#"# Add your Python dependencies here
# Example:
# requests>=2.28.0
# click>=8.0.0
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("README.md"),
                    content: r#"# {{project_name}}

A Python command-line application.

## Installation

```bash
pip install -r requirements.txt
```

## Usage

```bash
python main.py --help
python main.py --input input.txt --output output.txt --verbose
```

## Development

```bash
# Install development dependencies
pip install -r requirements.txt

# Run the application
python main.py

# Run tests (if you add pytest)
pytest
```
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from(".gitignore"),
                    content: r#"__pycache__/
*.py[cod]
*$py.class
*.so
.Python
build/
develop-eggs/
dist/
downloads/
eggs/
.eggs/
lib/
lib64/
parts/
sdist/
var/
wheels/
*.egg-info/
.installed.cfg
*.egg
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![
                "python -m py_compile main.py".to_string(),
            ],
            dependencies: vec!["python3".to_string()],
        }
    }

    fn create_node_app_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "node-app".to_string(),
            description: "Node.js application with modern JavaScript".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from("package.json"),
                    content: r#"{
  "name": "{{project_name}}",
  "version": "1.0.0",
  "description": "A Node.js application",
  "main": "index.js",
  "scripts": {
    "start": "node index.js",
    "dev": "nodemon index.js",
    "test": "jest"
  },
  "keywords": ["nodejs", "javascript"],
  "author": "Your Name",
  "license": "MIT",
  "dependencies": {
    "express": "^4.18.0"
  },
  "devDependencies": {
    "nodemon": "^3.0.0",
    "jest": "^29.0.0"
  }
}
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("index.js"),
                    content: r#"const express = require('express');
const app = express();
const port = process.env.PORT || 3000;

// Middleware
app.use(express.json());
app.use(express.urlencoded({ extended: true }));

// Routes
app.get('/', (req, res) => {
    res.json({ 
        message: 'Hello from {{project_name}}!',
        timestamp: new Date().toISOString()
    });
});

app.get('/health', (req, res) => {
    res.json({ status: 'OK', service: '{{project_name}}' });
});

// Start server
app.listen(port, () => {
    console.log(`{{project_name}} server running on port ${port}`);
});

module.exports = app;
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("README.md"),
                    content: r#"# {{project_name}}

A Node.js application.

## Installation

```bash
npm install
```

## Usage

```bash
# Development mode with auto-reload
npm run dev

# Production mode
npm start

# Run tests
npm test
```

## API Endpoints

- `GET /` - Welcome message
- `GET /health` - Health check

## Environment Variables

- `PORT` - Server port (default: 3000)
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from(".gitignore"),
                    content: r#"node_modules/
npm-debug.log*
yarn-debug.log*
yarn-error.log*
.env
.env.local
.env.development.local
.env.test.local
.env.production.local
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![
                "npm install".to_string(),
                "npm test".to_string(),
            ],
            dependencies: vec!["node".to_string(), "npm".to_string()],
        }
    }

    fn create_web_app_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "web-app".to_string(),
            description: "Simple HTML/CSS/JavaScript web application".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from("index.html"),
                    content: r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{{project_name}}</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <header>
        <h1>{{project_name}}</h1>
    </header>
    
    <main>
        <section class="container">
            <h2>Welcome</h2>
            <p>This is your new web application.</p>
            <button id="clickBtn">Click me!</button>
            <p id="message"></p>
        </section>
    </main>
    
    <footer>
        <p>&copy; 2024 {{project_name}}. All rights reserved.</p>
    </footer>
    
    <script src="script.js"></script>
</body>
</html>
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("style.css"),
                    content: r#"/* {{project_name}} Styles */

* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
    line-height: 1.6;
    color: #333;
    background-color: #f4f4f4;
}

header {
    background: #007acc;
    color: white;
    text-align: center;
    padding: 1rem 0;
    box-shadow: 0 2px 5px rgba(0,0,0,0.1);
}

header h1 {
    font-size: 2.5rem;
}

main {
    max-width: 1200px;
    margin: 2rem auto;
    padding: 0 1rem;
}

.container {
    background: white;
    padding: 2rem;
    border-radius: 8px;
    box-shadow: 0 2px 10px rgba(0,0,0,0.1);
    text-align: center;
}

.container h2 {
    color: #007acc;
    margin-bottom: 1rem;
}

.container p {
    margin-bottom: 1rem;
    font-size: 1.1rem;
}

button {
    background: #007acc;
    color: white;
    padding: 0.8rem 1.5rem;
    border: none;
    border-radius: 5px;
    cursor: pointer;
    font-size: 1rem;
    transition: background-color 0.3s;
}

button:hover {
    background: #005999;
}

#message {
    margin-top: 1rem;
    font-weight: bold;
    color: #007acc;
}

footer {
    background: #333;
    color: white;
    text-align: center;
    padding: 1rem 0;
    margin-top: 2rem;
}

@media (max-width: 768px) {
    header h1 {
        font-size: 2rem;
    }
    
    .container {
        margin: 1rem;
        padding: 1rem;
    }
}
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("script.js"),
                    content: r#"// {{project_name}} JavaScript

document.addEventListener('DOMContentLoaded', function() {
    const clickBtn = document.getElementById('clickBtn');
    const message = document.getElementById('message');
    let clickCount = 0;

    clickBtn.addEventListener('click', function() {
        clickCount++;
        
        const messages = [
            'Hello from {{project_name}}!',
            'You clicked the button!',
            `Click count: ${clickCount}`,
            'Keep clicking!',
            'This is interactive!'
        ];
        
        const randomMessage = messages[Math.floor(Math.random() * messages.length)];
        message.textContent = `${randomMessage} (Click #${clickCount})`;
        
        // Add some visual feedback
        clickBtn.style.transform = 'scale(0.95)';
        setTimeout(() => {
            clickBtn.style.transform = 'scale(1)';
        }, 100);
    });
    
    // Add fade-in animation
    document.body.style.opacity = '0';
    setTimeout(() => {
        document.body.style.transition = 'opacity 0.5s';
        document.body.style.opacity = '1';
    }, 100);
});

// Utility functions
function getCurrentTime() {
    return new Date().toLocaleTimeString();
}

function log(message) {
    console.log(`[${getCurrentTime()}] {{project_name}}: ${message}`);
}

// Initialize
log('Application loaded successfully');
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("README.md"),
                    content: r#"# {{project_name}}

A simple HTML/CSS/JavaScript web application.

## Features

- Responsive design
- Interactive JavaScript
- Modern CSS styling
- Mobile-friendly

## Usage

1. Open `index.html` in your web browser
2. Or serve it with a local server:

```bash
# Python
python -m http.server 8000

# Node.js (if you have http-server installed)
npx http-server

# PHP
php -S localhost:8000
```

Then visit http://localhost:8000

## File Structure

- `index.html` - Main HTML page
- `style.css` - Styling
- `script.js` - JavaScript functionality
- `README.md` - This file

## Customization

- Edit the HTML structure in `index.html`
- Modify styles in `style.css`
- Add functionality in `script.js`
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![],
            dependencies: vec![],
        }
    }

    fn create_config_files_template() -> ProjectTemplate {
        ProjectTemplate {
            name: "config-files".to_string(),
            description: "Common configuration files for development projects".to_string(),
            files: vec![
                FileTemplate {
                    path: PathBuf::from(".gitignore"),
                    content: r#"# Logs
logs
*.log
npm-debug.log*
yarn-debug.log*
yarn-error.log*

# Runtime data
pids
*.pid
*.seed
*.pid.lock

# Coverage directory used by tools like istanbul
coverage/

# nyc test coverage
.nyc_output

# Dependency directories
node_modules/
target/
__pycache__/

# Optional npm cache directory
.npm

# Environment variables
.env
.env.local
.env.development.local
.env.test.local
.env.production.local

# OS generated files
.DS_Store
.DS_Store?
._*
.Spotlight-V100
.Trashes
ehthumbs.db
Thumbs.db

# Editor directories and files
.vscode/
.idea/
*.swp
*.swo
*~

# Build outputs
dist/
build/
*.exe
*.dll
*.so
*.dylib
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from(".env.example"),
                    content: r#"# Environment Variables Template
# Copy this file to .env and fill in your values

# Application settings
APP_NAME={{project_name}}
APP_ENV=development
APP_PORT=3000

# Database (example)
# DATABASE_URL=postgresql://user:pass@localhost/db_name

# API Keys (example)
# API_KEY=your_api_key_here
# SECRET_KEY=your_secret_key_here

# External services (example)
# REDIS_URL=redis://localhost:6379
# SMTP_HOST=localhost
# SMTP_PORT=587
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
                FileTemplate {
                    path: PathBuf::from("LICENSE"),
                    content: r#"MIT License

Copyright (c) 2024 {{project_name}}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"#.to_string(),
                    permissions: None,
                    is_executable: false,
                },
            ],
            commands: vec![],
            dependencies: vec![],
        }
    }
}

impl ProjectGenerator for ProjectTemplateManager {
    fn generate_project(&self, template_name: &str, project_name: &str, target_dir: &PathBuf) -> Result<Vec<PathBuf>> {
        let template = self.templates.get(template_name)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found", template_name))?;

        let mut created_files = Vec::new();
        let mut file_manager = crate::file_manager::SystemFileManager::new(target_dir.join("backups"))?;

        // Create target directory if it doesn't exist
        if !target_dir.exists() {
            std::fs::create_dir_all(target_dir)?;
        }

        // Generate files from template
        for file_template in &template.files {
            let file_path = target_dir.join(&file_template.path);
            
            // Create parent directories if needed
            if let Some(parent) = file_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            // Replace template variables
            let content = file_template.content
                .replace("{{project_name}}", project_name);

            // Create the file
            let result = file_manager.execute_operation(
                crate::ai_operations::FileOperation::Create {
                    path: file_path.clone(),
                    content,
                }
            )?;

            if result.success {
                created_files.push(file_path.clone());

                // Set permissions if specified
                #[cfg(unix)]
                if let Some(mode) = file_template.permissions {
                    use std::os::unix::fs::PermissionsExt;
                    let permissions = std::fs::Permissions::from_mode(mode);
                    std::fs::set_permissions(&file_path, permissions)?;
                }
            }
        }

        // Execute post-generation commands
        if !template.commands.is_empty() {
            println!("📦 Running post-generation commands...");
            let mut command_executor = crate::command_executor::SystemCommandExecutor::new();
            
            for command in &template.commands {
                let parts: Vec<&str> = command.split_whitespace().collect();
                if !parts.is_empty() {
                    let request = crate::ai_operations::CommandRequest {
                        command: parts[0].to_string(),
                        args: parts[1..].iter().map(|s| s.to_string()).collect(),
                        working_dir: Some(target_dir.clone()),
                        timeout_ms: Some(60000), // 1 minute timeout
                        environment: std::collections::HashMap::new(),
                        require_confirmation: false,
                    };

                    match command_executor.execute(request) {
                        Ok(response) => {
                            if response.success {
                                println!("   ✅ {}", command);
                            } else {
                                println!("   ⚠️  {} (failed: {})", command, response.error);
                            }
                        }
                        Err(e) => {
                            println!("   ❌ {} (error: {})", command, e);
                        }
                    }
                }
            }
        }

        Ok(created_files)
    }

    fn list_available_templates(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }

    fn get_template(&self, name: &str) -> Option<ProjectTemplate> {
        self.templates.get(name).cloned()
    }
}