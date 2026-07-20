import type { JSX as ReactJSX, ReactNode } from "react";
import type { BurokkuStyle } from "@burokku/runtime";

export { Fragment, jsx, jsxs } from "react/jsx-runtime";

export interface ElementProps {
  children?: ReactNode;
  style?: BurokkuStyle;
  id?: string;
  className?: string;
  onClick?: (event: Event) => void;
  [name: string]: unknown;
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
    [name: string]: ElementProps;
  }
}
