const text = { type: "string", value: "0" };
const root = {
  type: "app",
  children: [{
    type: "window",
    children: [{
      type: "flex",
      style: { flexDirection: "column", backgroundColor: "#f5f7fa" },
      children: [{
        type: "text",
        style: { color: "#18202b", fontSize: 42, lineHeight: 48 },
        children: [text],
      }],
    }],
  }],
};

let count = 0;
__burokku_render(root);
setInterval(() => {
  text.value = String(count++);
  __burokku_render(root);
}, 0);
