import type {
  AppNode,
  DivElement,
  GridElement,
  TextElement,
  TextNode,
  WindowElement,
} from "../src/index";

declare const app: AppNode;
declare const windowElement: WindowElement;
declare const div: DivElement;
declare const grid: GridElement;
declare const textElement: TextElement;
declare const nestedTextElement: TextElement;
declare const textNode: TextNode;

app.appendChild(windowElement);
windowElement.appendChild(div);
div.appendChild(grid);
div.appendChild(textElement);
textElement.appendChild(textNode);
textElement.appendChild(nestedTextElement);

textElement.textContent = "valid";
textNode.textContent = "valid";
textNode.nodeValue = "valid";
textNode.data = "valid";

// @ts-expect-error App accepts only Window elements.
app.appendChild(div);
// @ts-expect-error Ordinary containers reject raw text nodes.
windowElement.appendChild(textNode);
// @ts-expect-error Ordinary containers reject raw text nodes.
div.appendChild(textNode);
// @ts-expect-error Text elements reject ordinary element children.
textElement.appendChild(div);
// @ts-expect-error Raw text nodes are leaves.
textNode.appendChild(nestedTextElement);
// @ts-expect-error Non-text element textContent is read-only.
div.textContent = "invalid";
// @ts-expect-error Generic element nodeValue is read-only.
div.nodeValue = "invalid";
