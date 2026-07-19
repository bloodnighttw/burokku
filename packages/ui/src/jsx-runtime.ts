import type { JSX as ReactJSX } from "react";

import type { ElementProps } from "./index";

export { Fragment, jsx, jsxs } from "react/jsx-runtime";

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
    div: ElementProps;
    button: ElementProps;
    span: ElementProps;
    text: ElementProps;
  }
}
