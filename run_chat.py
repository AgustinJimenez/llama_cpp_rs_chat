#!/usr/bin/env python3
"""
Run the terminal chat from the project root directory.
This script allows you to run the chat from anywhere in the project.
"""
import sys
import os

# Add the python_test directory to the path
script_dir = os.path.dirname(os.path.abspath(__file__))
python_test_dir = os.path.join(script_dir, 'python_test')
sys.path.insert(0, python_test_dir)

# Import and run the chat
from terminal_chat import TerminalChat

if __name__ == "__main__":
    print("🚀 Starting Terminal LLM Chat from project root...")
    chat = TerminalChat()
    chat.run()