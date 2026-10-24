    // `CSS.escape` (the `CSS` namespace object itself, including `CSS.supports`,
    // is registered natively). Implements the CSSOM serialize-an-identifier
    // algorithm: https://drafts.csswg.org/cssom/#serialize-an-identifier
    if (typeof globalThis.CSS === "object" && typeof CSS.escape !== "function") {
        CSS.escape = function (value) {
            const string = String(value);
            const firstCodeUnit = string.charCodeAt(0);
            if (string.length === 1 && firstCodeUnit === 0x2d) {
                return "\\" + string;
            }
            let result = "";
            for (let index = 0; index < string.length; index++) {
                const codeUnit = string.charCodeAt(index);
                if (codeUnit === 0x0000) {
                    result += "\ufffd";
                } else if (
                    (codeUnit >= 0x0001 && codeUnit <= 0x001f) ||
                    codeUnit === 0x007f ||
                    (index === 0 && codeUnit >= 0x30 && codeUnit <= 0x39) ||
                    (index === 1 &&
                        codeUnit >= 0x30 &&
                        codeUnit <= 0x39 &&
                        firstCodeUnit === 0x2d)
                ) {
                    result += "\\" + codeUnit.toString(16) + " ";
                } else if (
                    codeUnit >= 0x0080 ||
                    codeUnit === 0x2d ||
                    codeUnit === 0x5f ||
                    (codeUnit >= 0x30 && codeUnit <= 0x39) ||
                    (codeUnit >= 0x41 && codeUnit <= 0x5a) ||
                    (codeUnit >= 0x61 && codeUnit <= 0x7a)
                ) {
                    result += string.charAt(index);
                } else {
                    result += "\\" + string.charAt(index);
                }
            }
            return result;
        };
    }
