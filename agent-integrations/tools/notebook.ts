import { readFile, writeFile } from "node:fs/promises";
import { extname, isAbsolute, resolve } from "node:path";

import type {
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";

export type NotebookCellType = "code" | "markdown";
export type NotebookEditMode = "replace" | "insert" | "delete";

export interface NotebookCell {
  id?: string;
  cell_type: NotebookCellType;
  source: string[] | string;
  metadata?: Record<string, unknown>;
  outputs?: unknown[];
  execution_count?: number | null;
}

export interface NotebookDocument {
  nbformat: number;
  nbformat_minor: number;
  metadata?: Record<string, unknown>;
  cells: NotebookCell[];
}

export interface NotebookEditInput {
  notebookPath: string;
  cellId?: string;
  newSource?: string;
  cellType?: NotebookCellType;
  editMode?: NotebookEditMode;
}

export interface NotebookEditOutput {
  tool: "NotebookEdit";
  notebookPath: string;
  cellId: string;
  cellType: NotebookCellType;
  language: string;
  editMode: NotebookEditMode;
  newSource: string;
  cellIndex: number;
  totalCells: number;
  originalFile: string;
  updatedFile: string;
}

export interface NotebookFileSystem {
  readText(path: string): Promise<string>;
  writeText(path: string, content: string): Promise<void>;
}

export class NodeNotebookFileSystem implements NotebookFileSystem {
  async readText(path: string): Promise<string> {
    return readFile(path, "utf8");
  }

  async writeText(path: string, content: string): Promise<void> {
    await writeFile(path, content, "utf8");
  }
}

export class NotebookEditTool {
  constructor(
    private readonly fileSystem: NotebookFileSystem = new NodeNotebookFileSystem(),
  ) {}

  async execute(
    input: NotebookEditInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<NotebookEditOutput>> {
    const output = await this.applyEdit(input);
    if (context && forwarder) {
      await forwarder.forward(context, output);
    }
    return {
      tool: "NotebookEdit",
      ok: true,
      output,
    };
  }

  async applyEdit(input: NotebookEditInput): Promise<NotebookEditOutput> {
    const notebookPath = resolveNotebookPath(input.notebookPath);
    if (extname(notebookPath) !== ".ipynb") {
      throw new Error("Notebook edits only support .ipynb files.");
    }

    const editMode = input.editMode ?? "replace";
    if (editMode !== "delete" && typeof input.newSource !== "string") {
      throw new Error("newSource is required for replace and insert edits.");
    }

    if (editMode === "insert" && !input.cellType) {
      throw new Error("cellType is required when inserting a new notebook cell.");
    }

    const originalFile = await this.fileSystem.readText(notebookPath);
    const notebook = parseNotebookDocument(originalFile);
    ensureCellIds(notebook);

    const targetIndex = input.cellId
      ? notebook.cells.findIndex(cell => cell.id === input.cellId)
      : -1;
    if (input.cellId && targetIndex === -1) {
      throw new Error(`Notebook cell ${input.cellId} was not found.`);
    }
    if ((editMode === "replace" || editMode === "delete") && targetIndex === -1) {
      throw new Error(`Notebook edit mode ${editMode} requires an existing cellId.`);
    }

    let cellIndex = targetIndex;
    let cellId = input.cellId ?? "";
    let cellType: NotebookCellType;

    switch (editMode) {
      case "replace": {
        const existingCell = notebook.cells[targetIndex];
        existingCell.cell_type = input.cellType ?? existingCell.cell_type;
        existingCell.source = toNotebookSource(input.newSource ?? "");
        cellType = existingCell.cell_type;
        cellId = existingCell.id ?? generateNotebookCellId();
        existingCell.id = cellId;
        break;
      }
      case "insert": {
        const insertIndex = targetIndex >= 0 ? targetIndex + 1 : 0;
        const newCell = createNotebookCell(input.cellType!, input.newSource ?? "");
        notebook.cells.splice(insertIndex, 0, newCell);
        cellIndex = insertIndex;
        cellId = newCell.id!;
        cellType = newCell.cell_type;
        break;
      }
      case "delete": {
        const [deletedCell] = notebook.cells.splice(targetIndex, 1);
        if (!deletedCell) {
          throw new Error(`Notebook cell ${input.cellId ?? ""} was not found.`);
        }
        cellType = deletedCell.cell_type;
        cellId = deletedCell.id ?? input.cellId ?? generateNotebookCellId();
        cellIndex = targetIndex;
        break;
      }
      default:
        throw new Error(`Unsupported notebook edit mode: ${String(editMode)}`);
    }

    const updatedFile = `${JSON.stringify(notebook, null, 2)}\n`;
    await this.fileSystem.writeText(notebookPath, updatedFile);

    return {
      tool: "NotebookEdit",
      notebookPath,
      cellId,
      cellType,
      language: getNotebookLanguage(notebook),
      editMode,
      newSource: editMode === "delete" ? "" : input.newSource ?? "",
      cellIndex,
      totalCells: notebook.cells.length,
      originalFile,
      updatedFile,
    };
  }
}

export function parseNotebookDocument(content: string): NotebookDocument {
  const parsed = JSON.parse(content) as Partial<NotebookDocument>;
  if (!parsed || !Array.isArray(parsed.cells)) {
    throw new Error("Notebook JSON is missing a cells array.");
  }
  return {
    nbformat: parsed.nbformat ?? 4,
    nbformat_minor: parsed.nbformat_minor ?? 5,
    metadata: parsed.metadata ?? {},
    cells: parsed.cells.map(cell => ({
      ...cell,
      cell_type: cell.cell_type,
      source: cell.source ?? [],
      metadata: cell.metadata ?? {},
    })),
  };
}

export function ensureCellIds(notebook: NotebookDocument): void {
  for (const cell of notebook.cells) {
    if (!cell.id) {
      cell.id = generateNotebookCellId();
    }
  }
}

export function createNotebookCell(
  cellType: NotebookCellType,
  source: string,
): NotebookCell {
  const baseCell: NotebookCell = {
    id: generateNotebookCellId(),
    cell_type: cellType,
    source: toNotebookSource(source),
    metadata: {},
  };
  if (cellType === "code") {
    return {
      ...baseCell,
      outputs: [],
      execution_count: null,
    };
  }
  return baseCell;
}

export function toNotebookSource(source: string): string[] {
  if (!source.length) {
    return [];
  }
  return source
    .replace(/\r\n/g, "\n")
    .split(/(?<=\n)/);
}

export function getNotebookLanguage(notebook: NotebookDocument): string {
  const metadata = notebook.metadata ?? {};
  const languageInfo = metadata["language_info"];
  if (languageInfo && typeof languageInfo === "object" && "name" in languageInfo) {
    const name = (languageInfo as Record<string, unknown>)["name"];
    if (typeof name === "string" && name.length > 0) {
      return name;
    }
  }
  const kernelspec = metadata["kernelspec"];
  if (kernelspec && typeof kernelspec === "object" && "language" in kernelspec) {
    const language = (kernelspec as Record<string, unknown>)["language"];
    if (typeof language === "string" && language.length > 0) {
      return language;
    }
  }
  return "python";
}

function generateNotebookCellId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID().replace(/-/g, "").slice(0, 12);
  }
  return `cell${Math.random().toString(16).slice(2, 10)}`;
}

function resolveNotebookPath(notebookPath: string): string {
  return isAbsolute(notebookPath) ? notebookPath : resolve(process.cwd(), notebookPath);
}
