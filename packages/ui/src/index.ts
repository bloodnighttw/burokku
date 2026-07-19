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

export interface SnapshotNode {
  id: number;
  type: ElementName;
  style: Record<string, unknown>;
  text?: string;
  children?: SnapshotNode[];
}

declare global {
  var __burokku_commit: ((snapshot: string) => void) | undefined;
}
