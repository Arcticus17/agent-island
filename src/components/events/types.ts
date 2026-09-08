import type { HookApproval } from "../../bridge/types";

export type CommandUiState = "idle" | "confirming" | "pending" | "succeeded" | "failed";

export interface CommandFailure {
  code: string;
  message: string;
  retryable: boolean;
}

export interface ApprovalItem {
  approval: HookApproval;
  state: CommandUiState;
  decision: boolean | null;
  error: CommandFailure | null;
}

export interface NotificationItem {
  id: string;
  agentName: string;
  sessionId: string;
  message: string;
  receivedAt: number;
  count: number;
}
