declare module "@tauri-apps/api/core" {
  export class Channel<T> {
    onmessage: (message: T) => void;
  }

  export function invoke<T>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T>;
}

interface Window {
  __TAURI_INTERNALS__?: unknown;
}
