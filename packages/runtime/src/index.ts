export type BurokkuStyle = Partial<{
  display: "block" | "flex" | "grid" | "inline" | "inline-block" | "inline-flex" | "inline-grid" | "none";
  position: "static" | "relative" | "absolute" | "fixed";
  top: number | string;
  right: number | string;
  bottom: number | string;
  left: number | string;
  zIndex: number | "auto";
  isolation: "auto" | "isolate";
  overflow: "visible" | "hidden" | "clip" | "auto" | "scroll";
  overflowX: "visible" | "hidden" | "clip" | "auto" | "scroll";
  overflowY: "visible" | "hidden" | "clip" | "auto" | "scroll";
  flexDirection: "row" | "column";
  flexWrap: "nowrap" | "wrap" | "wrap-reverse";
  flex: number | string;
  flexBasis: number | string;
  width: number | string;
  height: number | string;
  minWidth: number | string;
  minHeight: number | string;
  maxWidth: number | string;
  maxHeight: number | string;
  flexGrow: number;
  flexShrink: number;
  order: number;
  alignItems: string;
  alignSelf: string;
  alignContent: string;
  justifyContent: string;
  gap: number | string;
  rowGap: number | string;
  columnGap: number | string;
  padding: number | string;
  paddingTop: number | string;
  paddingRight: number | string;
  paddingBottom: number | string;
  paddingLeft: number | string;
  margin: number | string;
  marginTop: number | string;
  marginRight: number | string;
  marginBottom: number | string;
  marginLeft: number | string;
  gridTemplate: string;
  gridTemplateRows: string;
  gridTemplateColumns: string;
  gridTemplateAreas: string;
  gridAutoRows: string;
  gridAutoColumns: string;
  gridAutoFlow: string;
  gridRow: string;
  gridColumn: string;
  gridArea: string;
  backgroundColor: string;
  backgroundImage: string;
  color: string;
  borderColor: string;
  borderTopColor: string;
  borderRightColor: string;
  borderBottomColor: string;
  borderLeftColor: string;
  borderWidth: number | string;
  borderTopWidth: number | string;
  borderRightWidth: number | string;
  borderBottomWidth: number | string;
  borderLeftWidth: number | string;
  borderStyle: string;
  borderTopStyle: string;
  borderRightStyle: string;
  borderBottomStyle: string;
  borderLeftStyle: string;
  borderRadius: number | string;
  borderTopLeftRadius: number | string;
  borderTopRightRadius: number | string;
  borderBottomRightRadius: number | string;
  borderBottomLeftRadius: number | string;
  outlineColor: string;
  outlineWidth: number | string;
  outlineOffset: number | string;
  opacity: number | string;
  transform: string;
  boxShadow: string;
  textShadow: string;
  fontSize: number | string;
  lineHeight: number | string;
  fontWeight: number | "normal" | "bold";
  fontFamily: string;
  fontStyle: "normal" | "italic" | "oblique";
  textAlign: "start" | "end" | "left" | "right" | "center" | "justify";
  letterSpacing: number | string;
  wordSpacing: number | string;
  textDecoration: string;
  textDecorationLine: string;
  whiteSpace: "normal" | "nowrap" | "pre" | "pre-wrap" | "pre-line" | "break-spaces";
  overflowWrap: "normal" | "break-word" | "anywhere";
  wordBreak: "normal" | "break-all" | "keep-all";
}>;

export type HostProps = Record<string, unknown> & {
  children?: unknown;
  style?: BurokkuStyle;
};

const eventName = (name: string): string => name.slice(2).toLowerCase();
const cssName = (name: string): string =>
  name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);

export function setStyles(
  element: HTMLElement,
  previous: BurokkuStyle | undefined,
  next: BurokkuStyle | undefined,
): void {
  const names = new Set([
    ...Object.keys(previous ?? {}),
    ...Object.keys(next ?? {}),
  ] as Array<keyof BurokkuStyle>);
  const style = element.style as unknown as Record<string, unknown> & CSSStyleDeclaration;
  for (const name of names) {
    const oldValue = previous?.[name];
    const value = next?.[name];
    if (oldValue === value) continue;
    if (value === undefined || value === null) style.removeProperty(cssName(name));
    else style[name] = String(value);
  }
}

export function setProperty(
  element: HTMLElement,
  name: string,
  value: unknown,
  previous?: unknown,
): void {
  if (name === "children" || name === "ref" || name === "key") return;
  if (name === "style") {
    setStyles(element, previous as BurokkuStyle | undefined, value as BurokkuStyle | undefined);
    return;
  }
  if (name.startsWith("on") && name.length > 2) {
    const event = eventName(name);
    if (typeof previous === "function") {
      element.removeEventListener(event, previous as EventListener);
    }
    if (typeof value === "function") element.addEventListener(event, value as EventListener);
    return;
  }
  if (name === "className") name = "class";
  if (value === undefined || value === null || value === false) {
    element.removeAttribute(name);
  } else if (name in element && name !== "class") {
    (element as unknown as Record<string, unknown>)[name] = value;
  } else {
    element.setAttribute(name, value === true ? "" : String(value));
  }
}

export function updateProperties(
  element: HTMLElement,
  previous: HostProps,
  next: HostProps,
): void {
  const names = new Set([...Object.keys(previous), ...Object.keys(next)]);
  for (const name of names) {
    if (previous[name] !== next[name]) setProperty(element, name, next[name], previous[name]);
  }
}
