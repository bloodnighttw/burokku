(function() {
	//#region src/main.js
	/**
	* @param {{ readonly style: import("@burokku/runtime").BurokkuStyleDeclaration }} node
	* @param {Readonly<Record<string, string>>} declarations
	*/
	function setNativeStyles(node, declarations) {
		for (const [property, value] of Object.entries(declarations)) node.style.setProperty(property, value);
	}
	/**
	* @param {string} content
	* @param {string} testId
	* @param {Readonly<Record<string, string>>} [declarations]
	* @returns {import("@burokku/runtime").TextElement}
	*/
	function makeText(content, testId, declarations = {}) {
		const node = app.createElement("text");
		node.setAttribute("data-testid", testId);
		node.appendChild(app.createTextNode(content));
		setNativeStyles(node, {
			"font-family": "Noto Sans",
			"font-size": "16px",
			color: "#e5e7ebff",
			"line-height": "1.3",
			"text-wrap": "wrap",
			...declarations
		});
		return node;
	}
	/**
	* @param {string} id
	* @param {number} grow
	* @param {string} color
	* @param {string} heading
	* @param {string} body
	* @returns {import("@burokku/runtime").FlexElement}
	*/
	function makeCard(id, grow, color, heading, body) {
		const card = app.createElement("flex");
		card.setAttribute("data-testid", id);
		setNativeStyles(card, {
			"flex-direction": "column",
			"flex-basis": "0px",
			"flex-grow": String(grow),
			"flex-shrink": "1",
			padding: "16px",
			gap: "8px",
			"background-color": color
		});
		const title = makeText(heading, `${id}-title`, {
			"font-size": "18px",
			"font-weight": "bold",
			"text-wrap": "nowrap"
		});
		const copy = makeText(body, `${id}-body`, { "flex-grow": "1" });
		card.appendChild(title);
		card.appendChild(copy);
		return card;
	}
	const mainWindow = app.createElement("window");
	mainWindow.setAttribute("data-testid", "main-window");
	setNativeStyles(mainWindow, { "background-color": "#0f172aff" });
	const shell = app.createElement("flex");
	shell.setAttribute("data-testid", "shell");
	setNativeStyles(shell, {
		width: "100%",
		height: "100%",
		"flex-direction": "column",
		padding: "24px",
		gap: "16px",
		"background-color": "#111827ff"
	});
	const title = makeText("Burokku flex + text layout", "page-title", {
		"font-size": "30px",
		"font-weight": "bold",
		color: "#ffffffff",
		"text-wrap": "nowrap"
	});
	const subtitle = app.createElement("text");
	subtitle.setAttribute("data-testid", "subtitle");
	setNativeStyles(subtitle, {
		"font-family": "Noto Sans",
		"font-size": "16px",
		color: "#94a3b8ff",
		"line-height": "1.4",
		"text-wrap": "wrap"
	});
	subtitle.appendChild(app.createTextNode("Three flex children use a "));
	const ratio = app.createElement("text");
	ratio.setAttribute("data-testid", "subtitle-ratio");
	setNativeStyles(ratio, {
		"font-weight": "bold",
		color: "#fbbf24ff"
	});
	ratio.appendChild(app.createTextNode("1 : 2 : 1"));
	subtitle.appendChild(ratio);
	subtitle.appendChild(app.createTextNode(" grow ratio, while this sentence exercises inherited styled runs."));
	const cardRow = app.createElement("flex");
	cardRow.setAttribute("data-testid", "card-row");
	setNativeStyles(cardRow, {
		width: "100%",
		"flex-basis": "0px",
		"flex-grow": "1",
		"flex-shrink": "1",
		"flex-direction": "row",
		"align-items": "stretch",
		gap: "12px",
		"background-color": "#1f2937ff"
	});
	cardRow.appendChild(makeCard("card-left", 1, "#7f1d1dff", "One share", "The narrow left card forces this longer sentence onto several shaped lines."));
	cardRow.appendChild(makeCard("card-center", 2, "#14532dff", "Two shares", "The center card receives twice the flex growth and therefore gives Parley a wider text constraint."));
	cardRow.appendChild(makeCard("card-right", 1, "#1e3a8aff", "One share", "The right card should match the left card width and preserve the same padding."));
	const footer = app.createElement("flex");
	footer.setAttribute("data-testid", "footer");
	setNativeStyles(footer, {
		height: "52px",
		"flex-direction": "row",
		"align-items": "center",
		"justify-content": "center",
		"background-color": "#312e81ff"
	});
	footer.appendChild(makeText("Raw app.createElement + app.createTextNode", "footer-label", {
		"font-size": "14px",
		color: "#c7d2feff",
		"text-wrap": "nowrap"
	}));
	shell.appendChild(title);
	shell.appendChild(subtitle);
	shell.appendChild(cardRow);
	shell.appendChild(footer);
	mainWindow.appendChild(shell);
	app.appendChild(mainWindow);
	//#endregion
})();

//# sourceMappingURL=app.js.map