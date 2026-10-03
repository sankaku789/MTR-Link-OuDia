import { invoke } from '@tauri-apps/api/core';

export type ErrorDto = { kind: string; message: string; detail?: string };
export type Endpoint = { url: string };
export type RouteStation = { station_name: string; platform_name: string; dwell_millis: number; run_millis_to_next?: number };
export type Route = { id: string; name: string; stations: RouteStation[]; station_count: number; total_run_millis: number; total_dwell_millis: number };
export type Snapshot = { routes: Route[]; dimensions: unknown[]; api_current_time_millis: number };
export type Inspection = {
  file_type: string; line_name?: string; station_count: number; kijun_status: string;
  diagrams: { index: number; train_count: number }[];
  train_types: number[];
  train_type_names: string[];
  templates: { diagram_index: number; direction: string; train_index: number; train_type_index?: number; active_station_slots: StationSlot[]; route_station_slots: StationSlot[] }[];
};
export type StationSlot = { index: number; name: string; handling_code?: number | null; previous_name?: string; next_name?: string };
export type Candidate = {
  id: string; diagram_index: number; train_index: number; direction: string; rank: string; reasons: string[]; auto_selected: boolean; manual_only: boolean;
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
export type OutboundStatus = {
  setting: { outbound_millis: number; measured_at: number; source: 'measured' | 'manual' } | null;
  valid: boolean; message: string | null; duration_label: string;
  first_station_name: string; first_platform_name: string;
};

function unavailable(error: unknown): never {
  if (typeof window !== 'undefined' && !('__TAURI_INTERNALS__' in window)) {
    throw new Error('Tauri デスクトップアプリで開いてください。ブラウザーでは変換を実行できません。');
  }
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const dto = error as Partial<ErrorDto>;
    const message = typeof dto.message === 'string' ? dto.message : '処理に失敗しました';
    const detail = typeof dto.detail === 'string' && dto.detail ? `（${dto.detail}）` : '';
    throw new Error(`${message}${detail}`);
  }
  throw error instanceof Error ? error : new Error(String(error));
}
async function call<T>(command: string, input: object): Promise<T> {
  try { return await invoke<T>(command, { input }); } catch (error) { return unavailable(error); }
}
async function callWithoutInput<T>(command: string): Promise<T> {
  try { return await invoke<T>(command); } catch (error) { return unavailable(error); }
}

export const api = {
  outboundStatus: (sessionId: string, routeId: string) => call<OutboundStatus>('get_outbound_status', { sessionId, routeId }),
  measureOutbound: (sessionId: string, routeId: string, depotClock: string, utcOffset: string) => call<OutboundStatus>('measure_outbound_runtime', { sessionId, routeId, depotClock, utcOffset }),
  manualOutbound: (sessionId: string, routeId: string, seconds: number) => call<OutboundStatus>('save_manual_outbound', { sessionId, routeId, seconds }),
  createSession: () => callWithoutInput<string>('create_conversion_session'),
  autoDetectionSupported: () => callWithoutInput<boolean>('is_mtr_auto_detection_supported'),
  detect: (sessionId?: string) => call<[string, Endpoint[]]>('detect_mtr_endpoints', { sessionId }),
  snapshot: (sessionId: string, endpoint: string, dimension: number) =>
    call<Snapshot>('fetch_mtr_snapshot', { sessionId, endpoint, dimension }),
  inspect: async (sessionId: string, path: string) => {
    const inspection = await call<Inspection | null>('inspect_oudia', { sessionId, path });
    if (!inspection) throw new Error('OuDiaの解析結果を取得できませんでした。');
    return inspection;
  },
  candidates: (sessionId: string, routeId: string, diagramIndex?: number, trainType?: number) =>
    call<Candidate[]>('find_route_candidates', { sessionId, routeId, diagramIndex, trainType }),
  preview: (sessionId: string, candidateId: string, manualMappings?: { mtr_station_index: number; oudia_station_slot: number }[]) =>
    call<Preview>('build_preview', { sessionId, candidateId, manualMappings: manualMappings ? { station_mappings: manualMappings } : undefined }),
  save: (sessionId: string, previewId: string, outputPath: string, policy: string) =>
    call<SaveReceipt>('save_conversion', { sessionId, previewId, outputPath, policy })
};
