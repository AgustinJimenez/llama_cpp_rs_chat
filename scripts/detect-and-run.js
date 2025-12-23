#!/usr/bin/env node
/**
 * Cross-platform auto-detection script for optimal GPU acceleration
 * Detects OS and runs the appropriate platform-specific detection script
 */

import { spawn } from 'child_process';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

// Get current file path in ES modules
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

function log(icon, message, color = '') {
    const colors = {
        red: '\x1b[31m',
        green: '\x1b[32m',
        yellow: '\x1b[33m',
        blue: '\x1b[34m',
        cyan: '\x1b[36m',
        reset: '\x1b[0m'
    };
    console.log(`${colors[color] || ''}${icon} ${message}${colors.reset}`);
}

function detectPlatform() {
    const platform = process.platform;
    const arch = process.arch;
    
    log('🔍', 'Auto-detecting optimal GPU acceleration setup...', 'cyan');
    log('🖥️', `Platform: ${platform} (${arch})`, 'blue');
    
    return { platform, arch };
}

function runScript(scriptPath, args = []) {
    return new Promise((resolve, reject) => {
        log('🚀', `Running: ${scriptPath}`, 'cyan');
        
        const child = spawn(scriptPath, args, {
            stdio: 'inherit',
            shell: true,
            cwd: path.dirname(__dirname) // Go up one level to project root
        });
        
        child.on('close', (code) => {
            if (code === 0) {
                resolve(code);
            } else {
                reject(new Error(`Script exited with code ${code}`));
            }
        });
        
        child.on('error', (err) => {
            reject(err);
        });
    });
}

async function main() {
    try {
        // Get mode from command line argument (web or desktop)
        const mode = process.argv[2] || 'web';
        if (!['web', 'desktop'].includes(mode)) {
            throw new Error(`Invalid mode: ${mode}. Use 'web' or 'desktop'`);
        }
        
        log('🎯', `Mode: ${mode.toUpperCase()}`, 'blue');
        
        const { platform } = detectPlatform();
        const projectRoot = path.dirname(__dirname);
        
        let scriptPath;
        
        switch (platform) {
            case 'darwin':
                scriptPath = path.join(projectRoot, 'dev_auto.sh');
                if (!fs.existsSync(scriptPath)) {
                    throw new Error(`Auto-detection script not found: ${scriptPath}`);
                }
                log('🍎', `Using macOS auto-detection script for ${mode}`, 'green');
                break;
                
            case 'win32':
                // Try PowerShell first, then fallback to bash script (Git Bash)
                const psScript = path.join(projectRoot, 'dev_auto.ps1');
                const bashScript = path.join(projectRoot, 'dev_auto.sh');
                
                if (fs.existsSync(psScript)) {
                    scriptPath = `powershell -ExecutionPolicy Bypass -File "${psScript}" ${mode}`;
                    log('🪟', `Using Windows PowerShell auto-detection script for ${mode}`, 'green');
                } else if (fs.existsSync(bashScript)) {
                    scriptPath = `${bashScript} ${mode}`;
                    log('🪟', `Using Windows Bash auto-detection script for ${mode}`, 'green');
                } else {
                    throw new Error('No Windows auto-detection script found');
                }
                break;
                
            case 'linux':
                scriptPath = path.join(projectRoot, 'dev_auto.sh');
                if (!fs.existsSync(scriptPath)) {
                    throw new Error(`Auto-detection script not found: ${scriptPath}`);
                }
                log('🐧', `Using Linux auto-detection script for ${mode}`, 'green');
                break;
                
            default:
                log('❓', `Unknown platform: ${platform}, falling back to CPU mode`, 'yellow');
                scriptPath = 'npm run dev';
                break;
        }
        
        console.log(''); // Empty line for spacing
        if (scriptPath.endsWith('.sh')) {
            await runScript(scriptPath, [mode]);
        } else {
            await runScript(scriptPath);
        }
        
    } catch (error) {
        log('❌', `Error: ${error.message}`, 'red');
        log('🔄', 'Falling back to CPU mode...', 'yellow');
        
        // Fallback to basic dev command
        try {
            await runScript('npm run dev');
        } catch (fallbackError) {
            log('💥', `Fallback failed: ${fallbackError.message}`, 'red');
            process.exit(1);
        }
    }
}

// Run if called directly
if (import.meta.url === `file://${process.argv[1]}`) {
    main();
}

export { detectPlatform, runScript, main };