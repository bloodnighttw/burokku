const parsed = JSON.parse('{"count":2,"drop":true}', (key, value) => {
    if (key === "drop") return undefined;
    if (key === "count") return value + 1;
    return value;
});
const array = JSON.parse("[1,2,3]", (key, value) => {
    if (key === "1") return undefined;
    return typeof value === "number" ? value * 10 : value;
});
[
    JSON.stringify(
        { parsed, ignored: true },
        (key, value) => (key === "ignored" ? undefined : value),
        2,
    ),
    `${array.length}:${array[0]}:${1 in array}:${array[2]}`,
];
