export type BurokkuTagName = "div" | "flex" | "grid" | "text" | "window";

export type BurokkuElement<Tag extends BurokkuTagName = BurokkuTagName> = HTMLElement & {
  readonly localName: Tag;
};

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

/** Create one of the element kinds supported by the native DOM plugin. */
export function createElement<Tag extends BurokkuTagName>(tag: Tag): BurokkuElement<Tag> {
  return document.createElement(tag) as BurokkuElement<Tag>;
}

/** Apply a checked set of native styles to an element. */
export function setStyles(element: HTMLElement, styles: Readonly<BurokkuStyle>): void {
  for (const [name, value] of Object.entries(styles)) {
    if (value === undefined) continue;
    const cssName = name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
    element.style.setProperty(cssName, String(value));
  }
}
