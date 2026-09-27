<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# atlas-engine-utils-template

Small, strict string templating for prompt and text assets.

Supported syntax:

- `{{ name }}` placeholder interpolation
- `{{{{` for a literal `{{`
- `}}}}` for a literal `}}`

The library is intentionally strict:

- parsing fails on malformed placeholders
- rendering fails on missing values
- rendering fails on duplicate values
- rendering fails on extra values not used by the template

## Example

```rust
use atlas_engine_utils_template::Template;
use atlas_engine_utils_template::render;

let template = Template::parse(
    "Hello, {{ name }}.\nLiteral braces: {{{{ and }}}}.\nMode: {{ mode }}",
)?;

let rendered = template.render([
    ("name", "Atlas Agent"),
    ("mode", "strict"),
])?;

assert_eq!(
    rendered,
    "Hello, Atlas Agent.\nLiteral braces: {{ and }}.\nMode: strict"
);

let one_shot = render("Hi {{ who }}!", [("who", "there")])?;
assert_eq!(one_shot, "Hi there!");
# Ok::<(), Box<dyn std::error::Error>>(())
```
