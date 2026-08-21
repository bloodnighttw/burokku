export type BurokkuTagName = "div" | "flex" | "grid" | "text" | "window";

export type BurokkuEventListener = (event: unknown) => void;

/** Shared behavior implemented by every Burokku-native node wrapper. */
export interface Node<AllowedChild = never> {
  readonly parentNode: BurokkuNode | null;
  readonly childNodes: readonly AllowedChild[];
  readonly firstChild: AllowedChild | null;
  readonly lastChild: AllowedChild | null;
  readonly nextSibling: BurokkuNode | null;
  readonly previousSibling: BurokkuNode | null;
  readonly isConnected: boolean;
  readonly textContent: string;
  readonly nodeValue: string | null;

  appendChild<Child extends AllowedChild>(child: Child): Child;
  insertBefore<Child extends AllowedChild>(
    child: Child,
    reference: AllowedChild | null,
  ): Child;
  removeChild<Child extends AllowedChild>(child: Child): Child;
  replaceChild<OldChild extends AllowedChild>(
    newChild: AllowedChild,
    oldChild: OldChild,
  ): OldChild;
  contains(other: BurokkuNode): boolean;
  addEventListener(type: string, callback: BurokkuEventListener): void;
  removeEventListener(type: string, callback: BurokkuEventListener): void;
}

/** The permanent, host-created application mount root. */
export interface AppNode extends Node<WindowElement> {
  createElement<Tag extends BurokkuTagName>(tag: Tag): BurokkuElement<Tag>;
  createTextNode(data: string): TextNode;
}

/** A raw DOM text node, distinct from the styled `text` element. */
export interface TextNode extends Node<never> {
  data: string;
  textContent: string;
  nodeValue: string;
}

/** The native style declaration exposed by a Burokku element. */
export interface BurokkuStyleDeclaration {
  supportsProperty(name: string): boolean;
  setProperty(name: string, value: string): void;
  removeProperty(name: string): void;
}

/** Shared behavior for script-creatable Burokku elements. */
export interface Element<
  Tag extends BurokkuTagName = BurokkuTagName,
  AllowedChild = BurokkuContainerChild,
> extends Node<AllowedChild> {
  readonly localName: Tag;
  readonly style: BurokkuStyleDeclaration;

  getAttribute(name: string): string | null;
  hasAttribute(name: string): boolean;
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

export interface DivElement extends Element<"div"> {}
export interface FlexElement extends Element<"flex"> {}
export interface GridElement extends Element<"grid"> {}
export interface TextElement extends Element<"text", BurokkuTextChild> {
  textContent: string;
}
export interface WindowElement extends Element<"window"> {}

/** Element children accepted by window and ordinary layout containers. */
export type BurokkuContainerChild =
  | DivElement
  | FlexElement
  | GridElement
  | TextElement;

/** Children accepted by a styled text element. */
export type BurokkuTextChild = TextNode | TextElement;

/** Any node exposed by the Burokku-native facade. */
export type BurokkuNode = AppNode | TextNode | BurokkuElement;

export interface BurokkuElementTagNameMap {
  div: DivElement;
  flex: FlexElement;
  grid: GridElement;
  text: TextElement;
  window: WindowElement;
}

export type BurokkuElement<
  Tag extends BurokkuTagName = BurokkuTagName,
> = BurokkuElementTagNameMap[Tag];

export type BurokkuDimension = "auto" | `${number}px` | `${number}%`;
export type BurokkuLength = `${number}px` | `${number}%`;
/** A CSS hexadecimal color: #rgb, #rgba, #rrggbb, or #rrggbbaa. */
export type BurokkuColor = `#${string}`;

/** Styles currently understood by Burokku's native layout bridge. */
export type BurokkuStyle = Partial<{
  width: BurokkuDimension;
  height: BurokkuDimension;
  padding: BurokkuLength;
  margin: BurokkuLength;
  backgroundColor: BurokkuColor;
  fontFamily: string;
  fontSize: `${number}px`;
  fontWeight: number | "normal" | "bold";
  color: BurokkuColor;
  lineHeight: "normal" | number | `${number}px`;
  textWrap: "wrap" | "nowrap";
  flexBasis: BurokkuDimension;
  flexGrow: number;
  flexShrink: number;
  flexDirection: "row" | "row-reverse" | "column" | "column-reverse";
  flexWrap: "nowrap" | "wrap" | "wrap-reverse";
  gap: BurokkuLength;
  columnGap: BurokkuLength;
  rowGap: BurokkuLength;
  alignContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
  alignItems: "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  alignSelf: "auto" | "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  justifyContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
  justifyItems: "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  justifySelf: "auto" | "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  gridRow: string;
  gridRowStart: string;
  gridRowEnd: string;
  gridColumn: string;
  gridColumnStart: string;
  gridColumnEnd: string;
  gridAutoFlow: "row" | "column" | "row dense" | "column dense" | "dense";
}>;

/** Apply a checked set of native styles to a Burokku element. */
export function setStyles(element: Element, styles: Readonly<BurokkuStyle>): void {
  for (const [name, value] of Object.entries(styles)) {
    if (value === undefined) continue;
    const nativeName = name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
    element.style.setProperty(nativeName, String(value));
  }
}

declare global {
  /** The permanent Burokku application mount root. */
  var app: AppNode;
}
