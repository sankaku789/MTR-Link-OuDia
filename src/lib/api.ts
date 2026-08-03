import { invoke } from '@tauri-apps/api/core';

export type ErrorDto = { kind: string; message: string; detail?: string };
export type Endpoint = { url: string };
export type Route = { id: string; name: string; stations: string[] };
export type Snapshot = { routes: Route[]; dimensions: unknown[] };
export type Inspection = {
  file_type: string; kijun_status: string;
  diagrams: { index: number; train_count: number }[];
  train_types: number[];
  templates: { diagram_index: number; direction: string; train_index: number; train_type_index?: number }[];
};
export type Candidate = {
  id: string; direction: string; rank: string; reasons: string[]; auto_selected: boolean;
  station_mappings: { mtr_station_index: number; oudia_station_slot: number }[];
};
export type Preview = {
  id: string; fixed_base_time: string; warnings: string[]; crosses_midnight: boolean;
  operation_present: boolean; policy_choices: string[];
  stops: {
    station: string; existing_arrival?: string; existing_departure?: string;
    raw_arrival_millis?: number; raw_departure_millis?: number;
    rounded_arrival?: string; rounded_departure?: string;
    run_millis?: number; dwell_millis: number;
  }[];
};
export type SaveReceipt = { output_path: string; bytes: number; sha256: string };

function unavailable(error: unknown): never {
  if (typeof window !== 'undefined' && !('__TAURI_INTERNALS__' in window)) {
    throw new Error('Tauri デスクトップアプリで開いてください。ブラウザーでは変換を実行できません。');
  }
  throw error;
}
async function call<T>(command: string, input: object): Promise<T> {
  try { return await invoke<T>(command, { input }); } catch (error) { return unavailable(error); }
}

export const api = {
  detect: (sessionId?: string) => call<[string, Endpoint[]]>('detect_mtr_endpoints', { sessionId }),
  snapshot: (sessionId: string, endpoint: string, dimension: number) =>
    call<Snapshot>('fetch_mtr_snapshot', { sessionId, endpoint, dimension }),
  inspect: (sessionId: string, path: string) => call<Inspection>('inspect_oudia', { sessionId, path }),
  candidates: (sessionId: string, routeId: string, diagramIndex?: number, trainType?: number) =>
    call<Candidate[]>('find_route_candidates', { sessionId, routeId, diagramIndex, trainType }),
  preview: (sessionId: string, candidateId: string) =>
    call<Preview>('build_preview', { sessionId, candidateId }),
  save: (sessionId: string, previewId: string, outputPath: string, policy: string) =>
    call<SaveReceipt>('save_conversion', { sessionId, previewId, outputPath, policy })
};
