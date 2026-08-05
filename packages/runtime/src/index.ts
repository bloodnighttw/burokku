export type ElementName = "window" | "div" | "flex" | "grid" | "text";

export type ColorValue = `#${string}`;

export interface GradientStop {
  color: ColorValue;
  position: number;
}

export type BackgroundImage =
  | {
      type: "linear-gradient";
      direction: readonly [number, number];
      stops: readonly GradientStop[];
    }
  | {
      type: "radial-gradient";
      stops: readonly GradientStop[];
    }
  | {
      type: "raster";
      width: number;
      height: number;
      pixels: readonly number[];
    };

export interface SharedPaintStyle {
  backgroundColor?: ColorValue;
  backgroundImage?: BackgroundImage;
  borderColor?: ColorValue;
  borderWidth?: number;
  borderRadius?: number;
}

export type FlexDirection = "row" | "row-reverse" | "column" | "column-reverse";
export type FlexWrap = "nowrap" | "wrap" | "wrap-reverse";
export type ItemAlignment =
  | "start"
  | "end"
  | "flex-start"
  | "flex-end"
  | "center"
  | "baseline"
  | "stretch";
export type ContentAlignment =
  | Exclude<ItemAlignment, "baseline">
  | "space-between"
  | "space-around"
  | "space-evenly";

export interface DivStyle extends SharedPaintStyle {}

export interface FlexStyle extends SharedPaintStyle {
  flexDirection?: FlexDirection;
  flexWrap?: FlexWrap;
  gap?: number;
  rowGap?: number;
  columnGap?: number;
  alignContent?: ContentAlignment;
  alignItems?: ItemAlignment;
  alignSelf?: ItemAlignment;
  justifyContent?: ContentAlignment;
  flexBasis?: number | "auto";
  flexGrow?: number;
  flexShrink?: number;
}

export type GridAutoFlow = "row" | "column" | "row-dense" | "column-dense";

export interface GridStyle extends SharedPaintStyle {
  gridTemplateColumns?: string;
  gridTemplateRows?: string;
  gridAutoColumns?: string;
  gridAutoRows?: string;
  gridAutoFlow?: GridAutoFlow;
  gap?: number;
  rowGap?: number;
  columnGap?: number;
  alignContent?: ContentAlignment;
  justifyContent?: ContentAlignment;
  alignItems?: ItemAlignment;
  justifyItems?: ItemAlignment;
  alignSelf?: ItemAlignment;
  justifySelf?: ItemAlignment;
  gridRow?: string;
  gridColumn?: string;
}

export type FontStyle = "normal" | "italic" | "oblique";
export type TextAlign = "start" | "end" | "left" | "right" | "center" | "justify";
export type TextDecorationLine =
  | "none"
  | "underline"
  | "overline"
  | "line-through"
  | "underline overline"
  | "underline line-through"
  | "overline line-through"
  | "underline overline line-through";
export type TextWhiteSpace =
  | "normal"
  | "nowrap"
  | "pre"
  | "pre-wrap"
  | "pre-line"
  | "break-spaces";
export type TextOverflowWrap = "normal" | "break-word" | "anywhere";
export type TextWordBreak = "normal" | "break-all" | "keep-all";

export interface TextStyle {
  color?: ColorValue;
  fontSize?: number;
  lineHeight?: number | "normal";
  fontWeight?: number | "normal" | "bold";
  fontFamily?: string;
  fontStyle?: FontStyle;
  textAlign?: TextAlign;
  letterSpacing?: number;
  wordSpacing?: number;
  textDecorationLine?: TextDecorationLine;
  textDecorationColor?: ColorValue;
  whiteSpace?: TextWhiteSpace;
  overflowWrap?: TextOverflowWrap;
  wordBreak?: TextWordBreak;
}

export interface ElementStyleMap {
  window: never;
  div: DivStyle;
  flex: FlexStyle;
  grid: GridStyle;
  text: TextStyle;
}

export type BurokkuStyle = DivStyle | FlexStyle | GridStyle | TextStyle;

export interface HostProps<Style extends BurokkuStyle = BurokkuStyle> {
  children?: unknown;
  style?: Style;
}

export interface HostRoot {
  readonly kind: "root";
  readonly autoCommit: boolean;
  readonly children: HostNode[];
  commitQueued: boolean;
}

export interface HostElement<Name extends ElementName = ElementName> {
  readonly kind: "element";
  readonly type: Name;
  parent: HostParent | null;
  readonly children: HostNode[];
  style?: ElementStyleMap[Name];
}

export interface HostText {
  readonly kind: "text";
  parent: HostParent | null;
  value: string;
}

export type HostNode = HostElement | HostText;
export type HostParent = HostRoot | HostElement;

export interface SerializedAppNode {
  type: "app";
  children: SerializedHostNode[];
}

export interface SerializedElementNode {
  type: ElementName;
  style?: BurokkuStyle;
  children: SerializedHostNode[];
}

export interface SerializedTextNode {
  type: "string";
  value: string;
}

export type SerializedHostNode = SerializedElementNode | SerializedTextNode;

declare global {
  // Installed by the native Burokku host before an application bundle runs.
  var __burokku_render: ((serializedTree: string) => void) | undefined;
}

export function createHostRoot(autoCommit = false): HostRoot {
  return {
    kind: "root",
    autoCommit,
    children: [],
    commitQueued: false,
  };
}

export function createHostElement<Name extends ElementName>(name: Name): HostElement<Name> {
  return {
    kind: "element",
    type: name,
    parent: null,
    children: [],
  };
}

export function createHostText(value: unknown): HostText {
  return {
    kind: "text",
    parent: null,
    value: String(value),
  };
}

export function setHostText(node: HostText, value: unknown): void {
  const next = String(value);
  if (node.value === next) return;
  node.value = next;
  queueConnectedCommit(node);
}

export function insertHostNode(
  parent: HostParent,
  node: HostNode,
  anchor: HostNode | null = null,
): void {
  if (anchor === node) return;
  if (anchor !== null && anchor.parent !== parent) {
    throw new Error("The host insertion anchor is not a child of the parent");
  }
  if (!acceptsChild(parent, node)) {
    throw new Error(`A ${describeParent(parent)} cannot contain ${describeNode(node)}`);
  }

  for (let ancestor: HostParent | null = parent; ancestor?.kind === "element"; ) {
    if (ancestor === node) throw new Error("A host node cannot contain itself");
    ancestor = ancestor.parent;
  }

  const previousParent = node.parent;
  const previousRoot = previousParent === null ? null : rootForParent(previousParent);
  let insertionIndex = anchor === null ? parent.children.length : parent.children.indexOf(anchor);

  if (previousParent !== null) {
    const previousIndex = previousParent.children.indexOf(node);
    if (previousIndex >= 0) {
      previousParent.children.splice(previousIndex, 1);
      if (previousParent === parent && previousIndex < insertionIndex) insertionIndex -= 1;
    }
  }

  parent.children.splice(insertionIndex, 0, node);
  node.parent = parent;

  const nextRoot = rootForParent(parent);
  queueRootCommit(previousRoot);
  if (nextRoot !== previousRoot) queueRootCommit(nextRoot);
}

export function removeHostNode(parent: HostParent, node: HostNode): void {
  const index = parent.children.indexOf(node);
  if (index < 0 || node.parent !== parent) {
    throw new Error("The host node to remove is not a child of the parent");
  }

  const root = rootForParent(parent);
  parent.children.splice(index, 1);
  node.parent = null;
  queueRootCommit(root);
}

export function getHostParentNode(node: HostNode): HostParent | undefined {
  return node.parent ?? undefined;
}

export function getHostFirstChild(parent: HostParent): HostNode | undefined {
  return parent.children[0];
}

export function getHostNextSibling(node: HostNode): HostNode | undefined {
  const parent = node.parent;
  if (parent === null) return undefined;
  const index = parent.children.indexOf(node);
  return index < 0 ? undefined : parent.children[index + 1];
}

export function setHostProperty(
  element: HostElement,
  name: string,
  value: unknown,
  _previous?: unknown,
): void {
  if (name === "children" || name === "ref" || name === "key") return;
  if (name !== "style") throw new Error(`Unsupported Burokku property '${name}'`);

  const next = cloneStyle(value);
  if (stylesEqual(element.style, next)) return;
  element.style = next;
  queueConnectedCommit(element);
}

export function updateHostProperties(
  element: HostElement,
  previous: HostProps,
  next: HostProps,
): void {
  const names = new Set([...Object.keys(previous), ...Object.keys(next)]);
  const previousRecord = previous as Record<string, unknown>;
  const nextRecord = next as Record<string, unknown>;
  for (const name of names) {
    if (previousRecord[name] !== nextRecord[name]) {
      setHostProperty(element, name, nextRecord[name], previousRecord[name]);
    }
  }
}

export function serializeHostRoot(root: HostRoot): SerializedAppNode {
  return {
    type: "app",
    children: root.children.map(serializeHostNode),
  };
}

export function commitHostRoot(root: HostRoot): void {
  root.commitQueued = false;
  const render = globalThis.__burokku_render;
  if (typeof render !== "function") {
    throw new Error("The native __burokku_render hook is not installed");
  }
  render(JSON.stringify(serializeHostRoot(root)));
}

function serializeHostNode(node: HostNode): SerializedHostNode {
  if (node.kind === "text") {
    return { type: "string", value: node.value };
  }

  const children = node.children.map(serializeHostNode);
  if (node.style === undefined || Object.keys(node.style).length === 0) {
    return { type: node.type, children };
  }
  return { type: node.type, style: node.style, children };
}

function cloneStyle(value: unknown): BurokkuStyle | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("The style property must be an object");
  }

  const style = Object.fromEntries(
    Object.entries(value).filter(([, entry]) => entry !== undefined && entry !== null),
  ) as BurokkuStyle;
  return Object.keys(style).length === 0 ? undefined : style;
}

function stylesEqual(previous: BurokkuStyle | undefined, next: BurokkuStyle | undefined): boolean {
  if (previous === next) return true;
  if (previous === undefined || next === undefined) return false;
  const previousKeys = Object.keys(previous);
  const nextKeys = Object.keys(next);
  return (
    previousKeys.length === nextKeys.length &&
    previousKeys.every((key) =>
      Object.is(
        (previous as Record<string, unknown>)[key],
        (next as Record<string, unknown>)[key],
      ),
    )
  );
}

function acceptsChild(parent: HostParent, child: HostNode): boolean {
  if (parent.kind === "root") {
    return child.kind === "element" && child.type === "window";
  }
  if (parent.type === "text") {
    return child.kind === "text" || (child.kind === "element" && child.type === "text");
  }
  return child.kind === "element" && child.type !== "window";
}

function describeParent(parent: HostParent): string {
  return parent.kind === "root" ? "app" : parent.type;
}

function describeNode(node: HostNode): string {
  return node.kind === "text" ? "string" : node.type;
}

function rootForParent(parent: HostParent): HostRoot | null {
  let current: HostParent | null = parent;
  while (current?.kind === "element") current = current.parent;
  return current;
}

function queueConnectedCommit(node: HostNode): void {
  if (node.parent !== null) queueRootCommit(rootForParent(node.parent));
}

function queueRootCommit(root: HostRoot | null): void {
  if (root === null || !root.autoCommit || root.commitQueued) return;
  root.commitQueued = true;
  void Promise.resolve().then(() => {
    if (root.commitQueued) commitHostRoot(root);
  });
}
