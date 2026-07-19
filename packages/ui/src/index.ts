import type { ReactNode } from "react";

export type ElementName = "div" | "button" | "span" | "text";
export type Color = `#${string}`;

export interface Style {
  display?: "block" | "flex" | "none";
  flexDirection?: "row" | "column";
  width?: number;
  height?: number;
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
  flexGrow?: number;
  flexShrink?: number;
  gap?: number;
  padding?: number;
  margin?: number;
  backgroundColor?: Color;
  color?: Color;
  borderColor?: Color;
  borderWidth?: number;
  borderRadius?: number;
  outlineColor?: Color;
  outlineWidth?: number;
  outlineOffset?: number;
  fontSize?: number;
  lineHeight?: number;
  fontWeight?: number;
  fontFamily?: string;
}

export interface ElementProps {
  style?: Style;
  children?: ReactNode;
}

declare global {
  var __burokku_create: ((id: number, type: ElementName) => void) | undefined;
  var __burokku_set_text: ((id: number, text: string) => void) | undefined;
  var __burokku_set_style_number:
    | ((id: number, name: keyof Style, value: number) => void)
    | undefined;
  var __burokku_set_style_string:
    | ((id: number, name: keyof Style, value: string) => void)
    | undefined;
  var __burokku_set_style_color:
    | ((
        id: number,
        name: keyof Style,
        red: number,
        green: number,
        blue: number,
        alpha: number,
      ) => void)
    | undefined;
  var __burokku_clear_style: ((id: number, name: keyof Style) => void) | undefined;
  var __burokku_insert:
    | ((parentId: number, childId: number, beforeId: number) => void)
    | undefined;
  var __burokku_remove: ((parentId: number, childId: number) => void) | undefined;
  var __burokku_flush: ((commitId: number) => void) | undefined;
  var __burokku_now: (() => number) | undefined;
}
