# Documents Library

This folder stores your raw source documents — papers, articles, notes, transcripts.

The AI agent can ingest these into the wiki knowledge base using WikiIngest.

## Folder Structure

```
documents/
  papers/        ← Research papers (PDF, markdown, text)
  articles/      ← Web articles, blog posts
  transcripts/   ← Meeting notes, chat exports
  downloads/     ← Files downloaded by the AI agent
```

## Supported File Types

- `.pdf` — Research papers (text extracted automatically via pypdf)
- `.md` — Markdown documents
- `.txt` — Plain text
- `.html` — Web pages
- `.json`, `.yaml`, `.yml` — Data files

## How to Use

### Step 1: Add files
Drop files into the appropriate subfolder, or use the GUI upload button.

### Step 2: Open the GUI
```bash
cd Homemade_AI_Agent_v1
python gui/server.py
# Open http://127.0.0.1:8420
```

### Step 3: Ingest into wiki
In the GUI:
1. Click **"Open Chat"** to enter the IDE
2. Click the **"Docs"** tab in the left panel
3. You'll see your files listed with **[View]** and **[Ingest]** buttons
4. Click **"Ingest"** on any file — the AI extracts it into wiki pages
5. Or click **"Ingest All Documents into Wiki"** to process everything at once

### Alternative: Use the chat
Just type in the chat panel:
> "Ingest the paper at documents/papers/my-paper.pdf into the wiki"

## PDF Support

PDF text extraction requires the `pypdf` package:
```bash
pip install pypdf
```
