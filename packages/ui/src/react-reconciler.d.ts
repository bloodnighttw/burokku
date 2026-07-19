declare module "react-reconciler" {
  interface ReconcilerInstance {
    createContainer(...args: unknown[]): unknown;
    updateContainer(element: unknown, container: unknown, parent: unknown, callback?: () => void): void;
    updateContainerSync?(element: unknown, container: unknown, parent: unknown, callback?: () => void): void;
    flushSyncWork?(): void;
  }

  export default function createReconciler(config: Record<string, unknown>): ReconcilerInstance;
}
