new Promise(resolve => {
    const order = [];
    let count = 0;

    setInterval(() => {
        count += 1;
        order.push(`interval-${count}`);

        if (count === 1) {
            Promise.resolve().then(() => order.push("microtask"));
        }

        if (count === 3) {
            resolve(order.join(","));
        }
    }, 5);
})
