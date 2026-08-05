import type { JSX as ReactJSX, Key, ReactNode } from "react";
import type { DivStyle, FlexStyle, GridStyle, TextStyle } from "@burokku/runtime";

export { Fragment, jsx, jsxs } from "react/jsx-runtime";

export interface ElementProps<Style> {
  key?: Key;
  children?: ReactNode;
  style?: Style;
}

export interface WindowProps {
  key?: Key;
  children?: ReactNode;
  style?: never;
}

export namespace JSX {
  export type Element = ReactJSX.Element;
  export interface ElementClass extends ReactJSX.ElementClass {}
  export interface ElementAttributesProperty extends ReactJSX.ElementAttributesProperty {}
  export interface ElementChildrenAttribute extends ReactJSX.ElementChildrenAttribute {}
  export type LibraryManagedAttributes<Component, Props> = ReactJSX.LibraryManagedAttributes<
    Component,
    Props
  >;
  export interface IntrinsicAttributes extends ReactJSX.IntrinsicAttributes {}
  export interface IntrinsicClassAttributes<T> extends ReactJSX.IntrinsicClassAttributes<T> {}
  export interface IntrinsicElements {
    window: WindowProps;
    div: ElementProps<DivStyle>;
    flex: ElementProps<FlexStyle>;
    grid: ElementProps<GridStyle>;
    text: ElementProps<TextStyle>;
  }
}
