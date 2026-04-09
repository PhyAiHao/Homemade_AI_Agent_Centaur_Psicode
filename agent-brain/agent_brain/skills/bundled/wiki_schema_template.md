# Wiki Schema

## Page Types
- **entity**: A specific thing (person, project, tool, API, service)
- **concept**: An abstract idea (design pattern, principle, methodology)
- **summary**: Condensed version of a source document
- **comparison**: Side-by-side analysis of two or more things
- **log**: Chronological record (ingests, decisions, incidents)
- **synthesis**: LLM-generated answer to a complex question

## Conventions
- Titles: noun phrases, title case ("Auth Migration Decision")
- One concept per page. Split if body exceeds 50 lines.
- Cross-reference with `[[slug]]` syntax in page bodies
- Tags: lowercase, hyphenated (`#api-design`, `#q3-2026`)
- Dates: always absolute (`2026-04-08`), never relative

## Memory Types
- `user`: role, preferences, expertise, working style
- `feedback`: how the agent should behave, what to avoid or repeat
- `project`: deadlines, initiatives, incidents, team context
- `reference`: pointers to external systems, dashboards, docs

## Tier Guidance
- **core** (max 10 files): user preferences, behavior feedback, active project context
- **archive** (unlimited): everything else (historical decisions, old bugs, references)
- `user` and `feedback` types default to core; `project` and `reference` to archive

## Ingest Workflow
1. Read the source fully
2. Create a summary page (`page_type: summary`)
3. Create or update entity pages for key nouns
4. Create or update concept pages for key ideas
5. Cross-reference with `[[slug]]` in page bodies
6. Append to `wiki/log.md`

## Quality Rules
- Every claim should cite its source (`source_url` in frontmatter)
- Flag contradictions: "Note: contradicts [[other-page]]"
- Prefer updating existing pages over creating duplicates
- Delete or merge when two pages cover the same topic
- Keep core pages concise (max 50 lines, max 3KB)

## Source Lineage
- `source_type`: how this knowledge was acquired
  - `web`: fetched from a URL
  - `file`: read from a local file
  - `transcript`: extracted from a conversation
  - `dream`: created by dream consolidation
  - `manual`: written directly by the user or LLM
- `source_url`: the URL or file path of the original source
