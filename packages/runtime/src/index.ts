export type BurokkuStyle = Partial<{
  display: "block" | "flex" | "none";
  flexDirection: "row" | "column";
  width: number | string;
  height: number | string;
  minWidth: number | string;
  minHeight: number | string;
  maxWidth: number | string;
  maxHeight: number | string;
  flexGrow: number;
  flexShrink: number;
  gap: number | string;
  padding: number | string;
  margin: number | string;
  backgroundColor: string;
  color: string;
  borderColor: string;
  borderWidth: number | string;
  borderRadius: number | string;
  outlineColor: string;
  outlineWidth: number | string;
  outlineOffset: number | string;
  fontSize: number | string;
  lineHeight: number | string;
  fontWeight: number | "normal" | "bold";
  fontFamily: string;
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
