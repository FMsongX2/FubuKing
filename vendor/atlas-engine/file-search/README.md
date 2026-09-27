<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# atlas_engine_file_search

Fast fuzzy file search tool for Atlas Agent.

Uses <https://crates.io/crates/ignore> under the hood (which is what `ripgrep` uses) to traverse a directory (while honoring `.gitignore`, etc.) to produce the list of files to search and then uses <https://crates.io/crates/nucleo-matcher> to fuzzy-match the user supplied `PATTERN` against the corpus.
