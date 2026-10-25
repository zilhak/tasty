export type NotifyCursor = {
  namespace: string;
  next: number;
  retention: number;
  missing: boolean;
};
declare module 'claude-code' {
  interface PluginState {
    'tasty-notify': { cursor: NotifyCursor };
  }
}
