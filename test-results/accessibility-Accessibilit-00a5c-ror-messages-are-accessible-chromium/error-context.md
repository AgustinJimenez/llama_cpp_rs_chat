# Page snapshot

```yaml
- generic [ref=e3]:
  - generic [ref=e4]:
    - button "Open sidebar" [ref=e6] [cursor=pointer]:
      - img [ref=e7] [cursor=pointer]
    - generic [ref=e8]:
      - button "New chat" [ref=e9] [cursor=pointer]:
        - img [ref=e10] [cursor=pointer]
      - button "Settings" [ref=e11] [cursor=pointer]:
        - img [ref=e12] [cursor=pointer]
  - generic [ref=e16]:
    - heading "🦙 LLaMA Chat" [level=1] [ref=e19]
    - generic [ref=e20]:
      - paragraph [ref=e24]: This should trigger an error
      - paragraph [ref=e26]: "Error: HTTP error! status: 500"
    - generic [ref=e28]:
      - textbox "Type your message..." [ref=e30]
      - button [disabled]:
        - img
```