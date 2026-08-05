import type { JSX as SolidJSX } from "solid-js";
import type {
  DivStyle,
  FlexStyle,
  GridStyle,
  HostProps,
  TextStyle,
} from "@burokku/runtime";

interface WindowProps {
  children?: SolidJSX.Element;
  style?: never;
}

export namespace JSX {
  export type Element = SolidJSX.Element;

  export interface ElementChildrenAttribute {
    children: {};
  }

  export interface IntrinsicElements {
    window: WindowProps;
    div: HostProps<DivStyle>;
    flex: HostProps<FlexStyle>;
    grid: HostProps<GridStyle>;
    text: HostProps<TextStyle>;
  }
}
