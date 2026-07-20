declare module "react-reconciler" {
  const createReconciler: (hostConfig: Record<string, unknown>) => any;
  export default createReconciler;
}
