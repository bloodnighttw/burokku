let count = 0;

setInterval(() => {
    count += 1;
    console.log(`[llrt] count = ${count}`);
}, 1_000);

setInterval(async () => {
    try {
        const response = await fetch("https://example.com");
        const body = await response.text();
        console.log(
            `[llrt] fetched example.com: status=${response.status}, bytes=${body.length}`,
        );
    } catch (error) {
        console.error(`[llrt] fetch failed: ${error}`);
    }
}, 3_000);

void 0;
