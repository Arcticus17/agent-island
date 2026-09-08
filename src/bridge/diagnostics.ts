import type { DiagnosticView } from "./types";
import { invoke } from "./tauri";

export interface DiagnosticsResponse {
  generated_at_ms: number;
  issues: DiagnosticView[];
}

export interface DiagnosticsBridge {
  getDiagnostics(): Promise<DiagnosticsResponse>;
  exportDiagnostics(destination: string): Promise<void>;
}

export type DiagnosticsInvoke = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNonnegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function copyDiagnostic(value: unknown): DiagnosticView | null {
  if (
    !isRecord(value) ||
    typeof value.adapter !== "string" ||
    typeof value.code !== "string" ||
    !isRecord(value.freshness) ||
    !isNonnegativeInteger(value.freshness.observed_at_ms) ||
    typeof value.freshness.stale !== "boolean" ||
    !(value.event_type === null || typeof value.event_type === "string") ||
    !isNonnegativeInteger(value.event_count) ||
    !isNonnegativeInteger(value.message_count) ||
    !(value.skipped_lines === undefined || isNonnegativeInteger(value.skipped_lines)) ||
    !(value.candidate_count === undefined || isNonnegativeInteger(value.candidate_count))
  ) {
    return null;
  }

  const diagnostic: DiagnosticView = {
    adapter: value.adapter,
    code: value.code,
    freshness: {
      observed_at_ms: value.freshness.observed_at_ms,
      stale: value.freshness.stale,
    },
    event_type: value.event_type,
    event_count: value.event_count,
    message_count: value.message_count,
  };

  if (value.skipped_lines !== undefined) {
    diagnostic.skipped_lines = value.skipped_lines;
  }
  if (value.candidate_count !== undefined) {
    diagnostic.candidate_count = value.candidate_count;
  }

  return diagnostic;
}

export function parseDiagnosticsResponse(payload: unknown): DiagnosticsResponse {
  if (
    !isRecord(payload) ||
    !isNonnegativeInteger(payload.generated_at_ms) ||
    !Array.isArray(payload.issues)
  ) {
    throw new Error("invalid_diagnostics_response");
  }

  const issues: DiagnosticView[] = [];
  for (const value of payload.issues) {
    const diagnostic = copyDiagnostic(value);
    if (diagnostic === null) throw new Error("invalid_diagnostics_response");
    issues.push(diagnostic);
  }

  return {
    generated_at_ms: payload.generated_at_ms,
    issues,
  };
}

const defaultInvoke: DiagnosticsInvoke = (command, args) => invoke<unknown>(command, args);

export function createDiagnosticsBridge(
  invokeCommand: DiagnosticsInvoke = defaultInvoke,
): DiagnosticsBridge {
  return {
    async getDiagnostics() {
      let payload: unknown;
      try {
        payload = await invokeCommand("get_diagnostics");
      } catch {
        throw new Error("diagnostics_fetch_failed");
      }
      return parseDiagnosticsResponse(payload);
    },

    async exportDiagnostics(destination) {
      if (typeof destination !== "string" || destination.trim().length === 0) {
        throw new Error("diagnostics_destination_empty");
      }
      try {
        await invokeCommand("export_diagnostics", { destination });
      } catch {
        throw new Error("diagnostics_export_failed");
      }
    },
  };
}

export const tauriDiagnosticsBridge = createDiagnosticsBridge();
