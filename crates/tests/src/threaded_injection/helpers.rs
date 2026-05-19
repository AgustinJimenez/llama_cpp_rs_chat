use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::token::LlamaToken;
use serde_json::json;

pub(super) fn build_large_prompt() -> String {
    let tools_json = r#"

# Tools

You have access to the following tools. To use a tool, output a tool call in the format shown.

## read_file
Read a file from the filesystem. Supports text files and PDFs.
Parameters:
- path (string, required): The file path to read
- summary (boolean): If true, return a GPU-summarized version for large files
- pages (string): Page range for PDF files (e.g. "1-5")

## write_file
Write content to a file, creating directories as needed.
Parameters:
- path (string, required): The file path to write to
- content (string, required): The content to write

## edit_file
Edit a file by replacing text.
Parameters:
- path (string, required): The file path to edit
- old_text (string, required): The text to find and replace
- new_text (string, required): The replacement text

## execute_command
Execute a shell command and return its output.
Parameters:
- command (string, required): The command to execute
- background (boolean): Run in background (for servers/daemons)
- timeout (integer): Timeout in seconds (default: 120)
- summary (boolean): If true, return a GPU-summarized version for large outputs

## list_directory
List files and directories at the given path.
Parameters:
- path (string, required): The directory path to list
- recursive (boolean): List recursively
- max_depth (integer): Maximum recursion depth

## search_files
Search for text patterns in files using regex.
Parameters:
- pattern (string, required): The regex pattern to search for
- path (string): The directory to search in (default: current directory)
- file_pattern (string): Glob pattern to filter files (e.g. "*.rs")
- context_lines (integer): Number of context lines around matches

## find_files
Find files by name pattern.
Parameters:
- pattern (string, required): The glob pattern to match (e.g. "**/*.rs")
- path (string): The directory to search in
- max_results (integer): Maximum number of results

## web_search
Search the web for information.
Parameters:
- query (string, required): The search query
- num_results (integer): Number of results to return (default: 5)

## web_fetch
Fetch a web page and return its text content.
Parameters:
- url (string, required): The URL to fetch
- use_htmd (boolean): Use HTMD for better HTML-to-text conversion

## take_screenshot
Take a screenshot of the current screen.
Parameters: none

## click_screen
Click at screen coordinates.
Parameters:
- x (integer, required): X coordinate
- y (integer, required): Y coordinate
- button (string): Mouse button ("left", "right", "middle")

## type_text
Type text using the keyboard.
Parameters:
- text (string, required): The text to type

## press_key
Press a keyboard key or key combination.
Parameters:
- key (string, required): The key to press (e.g. "Enter", "Ctrl+C")

## execute_python
Execute a Python script and return its output.
Parameters:
- code (string, required): The Python code to execute
"#;

    let system_prompt = format!(
        "You are a highly capable AI coding assistant with access to tools for file manipulation, \
        command execution, web search, and desktop automation. You help users with software \
        engineering tasks including writing code, debugging, creating projects, and system \
        administration.\n\n\
        When the user asks you to do something, think step by step about the best approach, \
        then use the available tools to accomplish the task. Always explain what you're doing \
        and show relevant output.\n\n\
        Important guidelines:\n\
        - Always check if files exist before reading them\n\
        - Create directories before writing files to new paths\n\
        - Use appropriate error handling in generated code\n\
        - Prefer standard libraries over third-party dependencies when possible\n\
        - Test code after writing it\n\
        - Keep responses focused and avoid unnecessary explanations\n\
        {tools_json}"
    );

    format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n\
        <|im_start|>user\nCreate a NimLang web app with Jester that has a CRUD for people (name, age, email). \
        Use an in-memory seq for storage. Put the project in E:/repo/tmp_project/nim_crud_test. \
        Show me how to run it.<|im_end|>\n\
        <|im_start|>assistant\n"
    )
}

pub(super) fn build_jinja_prompt(model: &LlamaModel) -> Result<String, String> {
    let template = model
        .chat_template(None)
        .map_err(|e| format!("No chat template in model: {e}"))?;
    let template_str = template
        .to_string()
        .map_err(|e| format!("Template UTF-8 error: {e}"))?;

    let processed = template_str
        .replace(".endswith(", " is endingwith(")
        .replace(".startswith(", " is startingwith(")
        .replace(".strip()", " | trim")
        .replace(".upper()", " | upper")
        .replace(".lower()", " | lower");

    let tools: Vec<serde_json::Value> = vec![
        json!({"type": "function", "function": {
            "name": "read_file", "description": "Read a file from the filesystem",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The file path to read"},
                "summary": {"type": "boolean", "description": "Return GPU-summarized version"},
                "pages": {"type": "string", "description": "Page range for PDF files"}
            }, "required": ["path"]}
        }}),
        json!({"type": "function", "function": {
            "name": "write_file", "description": "Write content to a file",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The file path to write to"},
                "content": {"type": "string", "description": "The content to write"}
            }, "required": ["path", "content"]}
        }}),
        json!({"type": "function", "function": {
            "name": "execute_command", "description": "Execute a shell command",
            "parameters": {"type": "object", "properties": {
                "command": {"type": "string", "description": "The command to execute"},
                "background": {"type": "boolean", "description": "Run in background"},
                "timeout": {"type": "integer", "description": "Timeout in seconds"},
                "summary": {"type": "boolean", "description": "Return summarized output"}
            }, "required": ["command"]}
        }}),
        json!({"type": "function", "function": {
            "name": "list_directory", "description": "List files in a directory",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "Directory path"},
                "recursive": {"type": "boolean", "description": "List recursively"},
                "max_depth": {"type": "integer", "description": "Max recursion depth"}
            }, "required": ["path"]}
        }}),
        json!({"type": "function", "function": {
            "name": "search_files", "description": "Search for text patterns in files",
            "parameters": {"type": "object", "properties": {
                "pattern": {"type": "string", "description": "Regex pattern to search"},
                "path": {"type": "string", "description": "Directory to search"},
                "file_pattern": {"type": "string", "description": "Glob filter"},
                "context_lines": {"type": "integer", "description": "Context lines"}
            }, "required": ["pattern"]}
        }}),
        json!({"type": "function", "function": {
            "name": "find_files", "description": "Find files by name pattern",
            "parameters": {"type": "object", "properties": {
                "pattern": {"type": "string", "description": "Glob pattern"},
                "path": {"type": "string", "description": "Directory to search"},
                "max_results": {"type": "integer", "description": "Max results"}
            }, "required": ["pattern"]}
        }}),
        json!({"type": "function", "function": {
            "name": "web_search", "description": "Search the web for information",
            "parameters": {"type": "object", "properties": {
                "query": {"type": "string", "description": "Search query"},
                "num_results": {"type": "integer", "description": "Number of results"}
            }, "required": ["query"]}
        }}),
        json!({"type": "function", "function": {
            "name": "web_fetch", "description": "Fetch a web page as text",
            "parameters": {"type": "object", "properties": {
                "url": {"type": "string", "description": "URL to fetch"},
                "use_htmd": {"type": "boolean", "description": "Use HTMD converter"}
            }, "required": ["url"]}
        }}),
        json!({"type": "function", "function": {
            "name": "execute_python", "description": "Execute Python code",
            "parameters": {"type": "object", "properties": {
                "code": {"type": "string", "description": "Python code to execute"}
            }, "required": ["code"]}
        }}),
        json!({"type": "function", "function": {
            "name": "take_screenshot", "description": "Take a screenshot of the screen",
            "parameters": {"type": "object", "properties": {}, "required": []}
        }}),
    ];

    #[derive(serde::Serialize)]
    struct Msg {
        role: String,
        content: String,
    }

    let messages = vec![
        Msg {
            role: "system".into(),
            content: "You are a helpful AI coding assistant with access to tools for file \
                manipulation, command execution, web search, and desktop automation. \
                You help users with software engineering tasks. Use the available tools \
                to accomplish tasks. Always explain what you're doing.".into(),
        },
        Msg {
            role: "user".into(),
            content: "Create a NimLang web app with Jester that has a CRUD for people \
                (name, age, email). Use an in-memory seq for storage. Put the project \
                in E:/repo/tmp_project/nim_crud_test. Show me how to run it.".into(),
        },
    ];

    let mut env = minijinja::Environment::new();
    env.add_function("raise_exception", |msg: String| -> Result<String, minijinja::Error> {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            msg,
        ))
    });
    env.add_function("strftime_now", |_fmt: String| -> String { "2026-05-09".to_string() });
    env.add_filter("tojson", |val: minijinja::Value| -> String {
        serde_json::to_string(&val).unwrap_or_else(|_| format!("{val}"))
    });

    env.add_template("chat_template", &processed)
        .map_err(|e| format!("Template parse error: {e}"))?;

    let ctx = minijinja::context! {
        messages => &messages,
        tools => &tools,
        documents => Vec::<serde_json::Value>::new(),
        add_generation_prompt => true,
        available_tools => &tools,
        bos_token => "",
        eos_token => "<|im_end|>",
        enable_thinking => false,
    };

    let template = env
        .get_template("chat_template")
        .map_err(|e| format!("Get template error: {e}"))?;

    template.render(&ctx).map_err(|e| format!("Render error: {e}"))
}

pub(super) fn build_large_tool_output() -> String {
    let mut output = String::from("<tool_response>\n");
    output.push_str(r#"import jester, json, strutils, sequtils, os

type
  Person = object
    id: int
    name: string
    age: int
    email: string

var
  people: seq[Person] = @[]
  nextId: int = 1

proc findPerson(id: int): int =
  for i, p in people:
    if p.id == id:
      return i
  return -1

proc toJson(p: Person): JsonNode =
  %*{"id": p.id, "name": p.name, "age": p.age, "email": p.email}

proc toJson(ps: seq[Person]): JsonNode =
  var arr = newJArray()
  for p in ps:
    arr.add(p.toJson())
  return arr

routes:
  get "/":
    resp "Welcome to People CRUD API"

  get "/people":
    resp Http200, $people.toJson(), "application/json"

  get "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      resp Http200, $people[idx].toJson(), "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

  post "/people":
    try:
      let body = parseJson(request.body)
      let person = Person(
        id: nextId,
        name: body["name"].getStr(),
        age: body["age"].getInt(),
        email: body["email"].getStr()
      )
      inc nextId
      people.add(person)
      resp Http201, $person.toJson(), "application/json"
    except:
      resp Http400, """{"error": "Invalid JSON body"}""", "application/json"

  put "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      try:
        let body = parseJson(request.body)
        if body.hasKey("name"): people[idx].name = body["name"].getStr()
        if body.hasKey("age"): people[idx].age = body["age"].getInt()
        if body.hasKey("email"): people[idx].email = body["email"].getStr()
        resp Http200, $people[idx].toJson(), "application/json"
      except:
        resp Http400, """{"error": "Invalid JSON body"}""", "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

  delete "/people/@id":
    let idx = findPerson(parseInt(@"id"))
    if idx >= 0:
      people.delete(idx)
      resp Http204, "", "application/json"
    else:
      resp Http404, """{"error": "Person not found"}""", "application/json"

# HTML template for the web interface
const htmlTemplate = """
<!DOCTYPE html>
<html>
<head>
    <title>People CRUD</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; }
        table { width: 100%; border-collapse: collapse; margin: 20px 0; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #4CAF50; color: white; }
        tr:nth-child(even) { background-color: #f2f2f2; }
        .btn { padding: 5px 10px; margin: 2px; cursor: pointer; border: none; border-radius: 3px; }
        .btn-edit { background-color: #2196F3; color: white; }
        .btn-delete { background-color: #f44336; color: white; }
        .btn-add { background-color: #4CAF50; color: white; padding: 10px 20px; font-size: 16px; }
        form { background: #f9f9f9; padding: 20px; border-radius: 5px; margin: 20px 0; }
        input { padding: 8px; margin: 5px 0; width: 100%; box-sizing: border-box; }
        label { font-weight: bold; }
    </style>
</head>
<body>
    <h1>People Management</h1>
    <div id="people-list"></div>
    <button class="btn btn-add" onclick="showForm()">Add Person</button>
    <div id="form-container" style="display:none">
        <form onsubmit="savePerson(event)">
            <input type="hidden" id="person-id">
            <label>Name:</label><input type="text" id="name" required>
            <label>Age:</label><input type="number" id="age" required>
            <label>Email:</label><input type="email" id="email" required>
            <button type="submit" class="btn btn-add">Save</button>
            <button type="button" class="btn" onclick="hideForm()">Cancel</button>
        </form>
    </div>
    <script>
        async function loadPeople() {
            const res = await fetch('/people');
            const people = await res.json();
            const html = '<table><tr><th>ID</th><th>Name</th><th>Age</th><th>Email</th><th>Actions</th></tr>' +
                people.map(p => '<tr><td>'+p.id+'</td><td>'+p.name+'</td><td>'+p.age+'</td><td>'+p.email+'</td>' +
                '<td><button class="btn btn-edit" onclick="editPerson('+p.id+')">Edit</button> ' +
                '<button class="btn btn-delete" onclick="deletePerson('+p.id+')">Delete</button></td></tr>').join('') +
                '</table>';
            document.getElementById('people-list').innerHTML = html;
        }
        loadPeople();
    </script>
</body>
</html>
"""
"#);
    output.push_str("\n</tool_response>");
    output
}

pub(super) fn eval_tokens(
    ctx: &mut llama_cpp_2::context::LlamaContext,
    batch: &mut LlamaBatch,
    tokens: &[LlamaToken],
    start: i32,
) {
    let total = tokens.len();
    for (ci, chunk) in tokens.chunks(512).enumerate() {
        batch.clear();
        for (j, &tok) in chunk.iter().enumerate() {
            let pos = start + (ci * 512 + j) as i32;
            batch.add(tok, pos, &[0], ci * 512 + j == total - 1).unwrap();
        }
        ctx.decode(batch).expect("decode failed");
    }
    ctx.synchronize();
}
