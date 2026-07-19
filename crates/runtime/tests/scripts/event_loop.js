async function runEventLoop() {
    const order = [];

    setTimeout(() => {
        order.push("macrotask-1");
        Promise.resolve().then(() => order.push("microtask"));
    }, 10);

    setTimeout(() => order.push("macrotask-2"), 10);

    await new Promise(resolve => setTimeout(resolve, 20));
    return order.join(",");
}

runEventLoop()
