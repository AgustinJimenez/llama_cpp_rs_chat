#!/usr/bin/env python3
import os
import sys
import subprocess
import re
import json
import time
from datetime import datetime
from llama_cpp import Llama

class TerminalChat:
    def __init__(self, model_path=None):
        self.model_path = model_path or self.find_model_path()
        self.llm = None
        self.conversation_history = []
        self.conversation_file = None
        self.system_prompt = None
        
    def find_model_path(self):
        """Try to find model path from assets directory or use default"""
        # Find the project root directory (where assets folder is)
        script_dir = os.path.dirname(os.path.abspath(__file__))
        project_root = os.path.dirname(script_dir)  # Go up one level from python_test
        model_path_file = os.path.join(project_root, "assets", "model_path.txt")
        
        if os.path.exists(model_path_file):
            with open(model_path_file, 'r') as f:
                path = f.read().strip()
                if os.path.exists(path):
                    return path
        
        # Default model path
        default_path = "E:\\.lmstudio\\models\\lmstudio-community\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf"
        if os.path.exists(default_path):
            return default_path
        
        print("Please provide the path to your GGUF model file:")
        path = input("Model path: ").strip()
        if not os.path.exists(path):
            print(f"Error: Model file not found at {path}")
            sys.exit(1)
        return path
    
    def initialize_model(self):
        """Initialize the llama.cpp model"""
        try:
            print(f"Loading model from: {self.model_path}")
            self.llm = Llama(
                model_path=self.model_path,
                n_ctx=32000,
                n_threads=8,
                verbose=False
            )
            print("Model loaded successfully!")
        except Exception as e:
            print(f"Error loading model: {e}")
            sys.exit(1)
    
    def execute_command(self, command):
        """Execute a system command and return the output"""
        try:
            print(f"\n🔧 Executing: {command}")
            result = subprocess.run(
                command,
                shell=True,
                capture_output=True,
                text=True,
                timeout=30
            )
            
            output = ""
            if result.stdout:
                output += f"STDOUT:\n{result.stdout}\n"
            if result.stderr:
                output += f"STDERR:\n{result.stderr}\n"
            if result.returncode != 0:
                output += f"Return code: {result.returncode}\n"
            
            print(f"📋 Command output:\n{output}")
            return output
            
        except subprocess.TimeoutExpired:
            return "ERROR: Command timed out after 30 seconds"
        except Exception as e:
            return f"ERROR: {str(e)}"
    
    def parse_commands(self, text):
        """Parse commands from LLM response using multiple patterns"""
        commands = []
        
        # Pattern 1: ```bash or ```cmd blocks
        bash_pattern = r'```(?:bash|cmd|shell|terminal)\s*\n(.*?)\n```'
        matches = re.findall(bash_pattern, text, re.DOTALL | re.IGNORECASE)
        for match in matches:
            commands.extend([cmd.strip() for cmd in match.split('\n') if cmd.strip()])
        
        # Pattern 2: EXECUTE: command
        exec_pattern = r'EXECUTE:\s*(.+?)(?:\n|$)'
        matches = re.findall(exec_pattern, text, re.IGNORECASE)
        commands.extend([cmd.strip() for cmd in matches])
        
        # Pattern 3: RUN: command
        run_pattern = r'RUN:\s*(.+?)(?:\n|$)'
        matches = re.findall(run_pattern, text, re.IGNORECASE)
        commands.extend([cmd.strip() for cmd in matches])
        
        return commands
    
    def get_system_prompt(self):
        """Generate system prompt that teaches the LLM how to use commands"""
        import platform
        
        system_info = {
            'os': platform.system(),
            'version': platform.version(),
            'machine': platform.machine(),
            'platform': platform.platform(),
            'python_version': platform.python_version(),
            'cwd': os.getcwd()
        }
        
        
        return f"""You are a helpful AI assistant with FULL SYSTEM COMMAND EXECUTION capabilities running on this machine:

SYSTEM INFORMATION:
- Operating System: {system_info['os']} ({system_info['platform']})
- Machine Architecture: {system_info['machine']}
- Python Version: {system_info['python_version']}
- Current Working Directory: {system_info['cwd']}

COMMAND EXECUTION CAPABILITIES:
You have the ability to execute ANY terminal/system commands to accomplish user goals. You are NOT limited - you can:
- Execute system commands directly
- Install software and packages
- Create, modify, and delete files
- Run scripts and programs
- Access system information
- Manage processes
- And perform any other system operations

COMMAND EXECUTION FORMATS:
1. Code blocks with bash/cmd/shell/terminal:
   ```bash
   command here
   ```

2. Direct execution commands:
   EXECUTE: command here
   RUN: command here

PLATFORM-SPECIFIC COMMANDS:
- Windows: Use `dir`, `type`, `copy`, `move`, `del`, PowerShell commands
- Unix/Linux/macOS: Use `ls`, `cat`, `cp`, `mv`, `rm`, bash commands
- Current platform is: {system_info['os']}

GUIDELINES:
1. Only execute commands when the user specifically requests something that requires them
2. Don't run commands for simple greetings, casual conversation, or general questions
3. Always explain what you're doing and why before executing commands
4. Use platform-appropriate commands for the current OS ({system_info['os']})
5. Execute commands to accomplish user goals efficiently
6. IMPORTANT: After commands execute, always explain the results and what they mean
7. Continue your response after command execution - don't stop abruptly
8. If you find files like README.md, examine their contents to provide complete answers
9. Be helpful but concise - avoid overly long responses for simple interactions

EXAMPLES FOR {system_info['os']}:
""" + ("""
- List files: ```bash
dir /a
```
- Create file: EXECUTE: echo "Hello World" > test.txt
- Check Python: RUN: python --version
- Install package: ```bash
pip install requests
```""" if system_info['os'] == 'Windows' else """
- List files: ```bash
ls -la
```
- Create file: EXECUTE: echo "Hello World" > test.txt
- Check Python: RUN: python3 --version
- Install package: ```bash
pip3 install requests
```""") + f"""

You are running with full system access on {system_info['os']}. Use this power responsibly to help the user achieve their goals."""
    
    def initialize_conversation_file(self):
        """Initialize conversation logging file"""
        timestamp = int(time.time())
        
        # Find the project root directory (where assets folder is)
        script_dir = os.path.dirname(os.path.abspath(__file__))
        project_root = os.path.dirname(script_dir)  # Go up one level from python_test
        conversations_dir = os.path.join(project_root, "assets", "conversations")
        
        # Create conversations directory if it doesn't exist
        if not os.path.exists(conversations_dir):
            os.makedirs(conversations_dir)
        
        self.conversation_file = os.path.join(conversations_dir, f"chat_{timestamp}.json")
        
        # Initialize conversation data with system prompt
        self.system_prompt = self.get_system_prompt()
        conversation_data = {
            "timestamp": timestamp,
            "datetime": datetime.now().isoformat(),
            "system_prompt": self.system_prompt,
            "command_execution_enabled": True,
            "messages": []
        }
        
        try:
            with open(self.conversation_file, 'w', encoding='utf-8') as f:
                json.dump(conversation_data, f, indent=2, ensure_ascii=False)
        except Exception as e:
            print(f"Warning: Failed to create conversation file: {e}")
            self.conversation_file = None
    
    def save_message(self, role, content, commands_executed=None):
        """Save a message to the conversation file"""
        if not self.conversation_file:
            return
        
        try:
            # Read current conversation
            with open(self.conversation_file, 'r', encoding='utf-8') as f:
                conversation_data = json.load(f)
            
            # Add new message
            message = {
                "timestamp": time.time(),
                "datetime": datetime.now().isoformat(),
                "role": role,
                "content": content
            }
            
            if commands_executed:
                message["commands_executed"] = commands_executed
            
            conversation_data["messages"].append(message)
            
            # Save back to file
            with open(self.conversation_file, 'w', encoding='utf-8') as f:
                json.dump(conversation_data, f, indent=2, ensure_ascii=False)
                
        except Exception as e:
            print(f"Warning: Could not save message to conversation file: {e}")
    
    def generate_response(self, user_input):
        """Generate response from the model with streaming and command execution"""
        # Build prompt with conversation history and system prompt
        system_prompt = self.get_system_prompt()
        prompt = system_prompt + "\n\n"
        
        for entry in self.conversation_history[-5:]:  # Keep last 5 exchanges
            prompt += f"User: {entry['user']}\nAssistant: {entry['assistant']}\n"
        prompt += f"User: {user_input}\nAssistant:"
        
        try:
            response_text = ""
            stream = self.llm(
                prompt,
                max_tokens=-1,
                temperature=0.7,
                top_p=0.9,
                stop=["User:", "\nUser:"],
                echo=False,
                stream=True
            )
            
            for output in stream:
                token = output['choices'][0]['text']
                print(token, end='', flush=True)
                response_text += token
            
            # Parse and execute commands from the response
            commands_executed = []
            commands = self.parse_commands(response_text)
            command_outputs = []
            
            for command in commands:
                if command:
                    output = self.execute_command(command)
                    command_outputs.append(f"Command: {command}\nOutput: {output}")
                    commands_executed.append({"command": command, "output": output})
            
            # If commands were executed, add their output to the response
            if command_outputs:
                response_text += "\n\n--- Command Results ---\n" + "\n".join(command_outputs)
                print("\n\n--- Command Results ---")
                for cmd_output in command_outputs:
                    print(cmd_output)
            
            # Save assistant message with commands
            self.save_message("assistant", response_text.strip(), commands_executed if commands_executed else None)
            
            return response_text.strip()
        except Exception as e:
            return f"Error generating response: {e}"
    
    def run(self):
        """Main chat loop"""
        print("=" * 50)
        print("Terminal LLM Chat with Command Execution")
        print("Type 'quit', 'exit', or 'q' to end the conversation")
        print("=" * 50)
        
        self.initialize_model()
        self.initialize_conversation_file()
        
        print(f"📁 Conversation will be saved to: {self.conversation_file}")
        print(f"🔧 Command execution: ENABLED")
        print("-" * 50)
        
        while True:
            try:
                user_input = input("\nYou: ").strip()
                
                if user_input.lower() in ['quit', 'exit', 'q']:
                    print("Goodbye!")
                    break
                
                
                if not user_input:
                    continue
                
                # Save user message
                self.save_message("user", user_input)
                
                print("Assistant: ", end="", flush=True)
                response = self.generate_response(user_input)
                print()  # New line after streaming response
                
                # Store conversation in memory
                self.conversation_history.append({
                    'user': user_input,
                    'assistant': response
                })
                
            except KeyboardInterrupt:
                print("\n\nGoodbye!")
                break
            except EOFError:
                print("\n\nGoodbye!")
                break
            except Exception as e:
                print(f"\nError: {e}")

if __name__ == "__main__":
    chat = TerminalChat()
    chat.run()