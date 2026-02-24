# Agent Test Plan

This folder contains test files for evaluating LLM agent capabilities.
Each test checks a specific skill: reading files, extracting data, reasoning across files, and writing output.

## How to Run

**IMPORTANT: Use Chrome DevTools MCP (`chrome-devtools-mcp`) for all browser-based testing. Do NOT use curl or direct API calls for chat interaction — always go through the UI at http://localhost:4000.**

Give the model the following prompt:

> Read the file `agent-tests/TEST_PLAN.md` and execute all tests listed there. For each test, read the input files, perform the task, and write the output to the specified output file. After completing all tests, report your results.

## Tests

### Test 1: Read & Summarize
- **Input:** `story_the_lighthouse.txt`
- **Task:** Read the story and write a 2-3 sentence summary.
- **Output:** `results/test1_summary.txt`
- **Evaluation:** Summary must mention the lighthouse keeper (Elena), the storm, and the rescue.

### Test 2: Extract Structured Data from Text
- **Input:** `invoice_rawtext.txt`
- **Task:** Extract all invoice fields into a structured JSON file.
- **Output:** `results/test2_invoice.json`
- **Expected fields:** `invoice_number`, `date`, `vendor_name`, `vendor_address`, `items` (array with `description`, `quantity`, `unit_price`, `total`), `subtotal`, `tax`, `grand_total`
- **Evaluation:** Compare against `expected/test2_invoice.json`

### Test 3: Answer Questions from JSON
- **Input:** `employees.json`
- **Task:** Read the JSON and answer these questions, writing answers to the output file:
  1. How many employees work in the Engineering department?
  2. What is the average salary across all employees?
  3. Who is the highest-paid employee and what is their role?
  4. List all employees hired before 2022.
- **Output:** `results/test3_answers.txt`
- **Evaluation:** Compare against `expected/test3_answers.txt`

### Test 4: Multi-File Reasoning
- **Input:** `weather_log.txt` + `crop_yields.json`
- **Task:** Read both files and determine which weather conditions correlated with the best and worst crop yields. Write a brief analysis.
- **Output:** `results/test4_analysis.txt`
- **Evaluation:** Must identify that high rainfall months had better yields, and the July drought caused the lowest yield.

### Test 5: Transform Format (Text to JSON)
- **Input:** `server_logs.txt`
- **Task:** Parse the log entries and convert them into a JSON array. Each entry should have: `timestamp`, `level`, `service`, `message`.
- **Output:** `results/test5_logs.json`
- **Evaluation:** Compare structure against `expected/test5_logs.json`. Must have exactly 8 entries.

### Test 6: Write a Document from Specs
- **Input:** `api_spec.json`
- **Task:** Read the API specification and generate a human-readable API documentation file in markdown format. Include endpoint, method, description, parameters, and example request/response for each endpoint.
- **Output:** `results/test6_api_docs.md`
- **Evaluation:** Must document all 3 endpoints with correct methods, paths, and parameter descriptions.

## Scoring

| Test | Skill Tested | Pass Criteria |
|------|-------------|---------------|
| 1 | File read + summarization | Mentions Elena, storm, rescue |
| 2 | Text parsing + JSON creation | All fields extracted correctly |
| 3 | JSON comprehension + math | All 4 answers correct |
| 4 | Multi-file reasoning | Identifies rainfall-yield correlation and July drought |
| 5 | Log parsing + format conversion | 8 valid JSON entries with correct fields |
| 6 | Spec reading + doc generation | All 3 endpoints documented with parameters |
