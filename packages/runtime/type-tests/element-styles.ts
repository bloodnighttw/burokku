import {
  setStyles,
  type BurokkuFlexStyle,
  type DivElement,
  type FlexElement,
  type GridElement,
  type TextElement,
  type WindowElement,
} from "../src/index";

declare const div: DivElement;
declare const flex: FlexElement;
declare const grid: GridElement;
declare const text: TextElement;
declare const windowElement: WindowElement;
declare const flexStyles: BurokkuFlexStyle;

const rect = div.getBoundingClientRect();
if (rect) {
  const width: number = rect.width;
  void width;
  // @ts-expect-error Calculated layout values are read-only.
  rect.width = 10;
}

setStyles(div, {
  width: "100%",
  padding: "8px",
  flexGrow: 1,
  gridColumn: "2",
});
setStyles(flex, {
  flexDirection: "column",
  gap: "8px",
  alignItems: "center",
});
setStyles(grid, {
  gridAutoFlow: "row dense",
  justifyItems: "stretch",
  rowGap: "4px",
});
setStyles(text, {
  fontFamily: "sans-serif",
  fontSize: "16px",
  color: "#fff",
  margin: "4px",
});
setStyles(windowElement, {
  width: "640px",
  height: "480px",
  backgroundColor: "#000",
});

// @ts-expect-error Flex container properties are unsupported by div elements.
setStyles(div, { flexDirection: "row" });
// @ts-expect-error A predeclared flex style is also incompatible with div elements.
setStyles(div, flexStyles);
// @ts-expect-error Grid container properties are unsupported by flex elements.
setStyles(flex, { gridAutoFlow: "column" });
// @ts-expect-error Flex container properties are unsupported by grid elements.
setStyles(grid, { flexWrap: "wrap" });
// @ts-expect-error Typography is supported only by text elements.
setStyles(div, { fontSize: "16px" });
// @ts-expect-error Text elements do not support grid container properties.
setStyles(text, { justifyItems: "center" });
// @ts-expect-error Window elements do not support regular box properties.
setStyles(windowElement, { padding: "8px" });
// @ts-expect-error Window elements do not support item properties.
setStyles(windowElement, { flexGrow: 1 });
