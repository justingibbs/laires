Rebuild:
cargo build --release

Reinstall:
cargo install --path .

Version:
laires --version

Open terminal GUI:
laires open

Scan the folder:
laires scan


---


Build the FileBufferManager and cross-file SceneMap. FileBufferManager coordinates multiple TextBuffer instances (one per story file from the manifest), 
  routes edits by file path, and provides aggregate search. Cross-file SceneMap adds file_path to SceneSpan so scenes track which file they belong to, with
   ordering across files driven by manifest order. Read the spec in context/laires-spec.md for details, and check the existing text_buffer.rs,             
  scene_map.rs, and manifest.rs for current interfaces. Plan first.