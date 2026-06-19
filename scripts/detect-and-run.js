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

function runScript(scriptPath, args = [], background = false) {
    return new Promise((resolve, reject) => {
        log('🚀', `Running: ${scriptPath} ${args.join(' ')}`, 'cyan');
        
        const options = {
            stdio: 'inherit',
            shell: true,
            cwd: path.dirname(__dirname) // Go up one level to project root
        };

        if (background) {
            options.detached = true;
            options.stdio = 'ignore'; // Ignore stdio for detached processes
        }

        const child = spawn(scriptPath, args, options);
        
        if (background) {
            child.unref(); // Allow the parent process to exit independently
            resolve(0); // Resolve immediately for background processes
        } else {
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
        }
    });
}

async function main() {
    try {
        // Get mode from command line argument (web or desktop)
        const mode = process.argv[2] || 'web';
        if (!['web', 'desktop'].includes(mode)) {
            throw new Error(`Invalid mode: ${mode}. Use 'web' or 'desktop'`);
        }
        
        // Extract additional arguments, e.g., --background
        const additionalArgs = process.argv.slice(3);
        const isBackground = additionalArgs.includes('--background');

        log('🎯', `Mode: ${mode.toUpperCase()}`, 'blue');
        if (isBackground) {
            log('⚙️', 'Running in background mode', 'blue');
        }

        const { platform } = detectPlatform();
        const projectRoot = path.dirname(__dirname);

        // Ensure cmake is available before any build/run scripts
        await runScript('npm run ensure-cmake');
        
        let scriptPath;
        let scriptArgs = [mode]; // Start with the mode argument
        
        switch (platform) {
            case 'darwin':
                scriptPath = path.join(projectRoot, 'dev_auto.sh');
                if (!fs.existsSync(scriptPath)) {
                    throw new Error(`Auto-detection script not found: ${scriptPath}`);
                }
                log('🍎', `Using macOS auto-detection script for ${mode}`, 'green');
                break;
                
            case 'win32':
                const psScript = path.join(projectRoot, 'dev_auto.ps1');
                const bashScript = path.join(projectRoot, 'dev_auto.sh');
                
                if (fs.existsSync(psScript)) {
                    scriptPath = `powershell -ExecutionPolicy Bypass -File "${psScript}" -Mode ${mode}`;
                    // PowerShell arguments are passed as named parameters, not positional
                    // The background flag will be handled by detect-and-run.js, not passed to ps1
                    scriptArgs = []; 
                    log('🪟', `Using Windows PowerShell auto-detection script for ${mode}`, 'green');
                } else if (fs.existsSync(bashScript)) {
                    scriptPath = `${bashScript}`;
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
                scriptPath = 'npm run dev:cpu';
                scriptArgs = []; 
                break;
        }
        
        console.log(''); // Empty line for spacing
        // Pass the background flag to runScript
        await runScript(scriptPath, scriptArgs, isBackground);
        
    } catch (error) {
        log('❌', `Error: ${error.message}`, 'red');
        log('🔄', 'Falling back to CPU mode...', 'yellow');
        
        try {
            await runScript('npm run dev:cpu');
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
