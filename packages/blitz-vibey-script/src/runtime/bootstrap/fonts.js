    // `document.fonts` (FontFaceSet) stub: all fonts report as loaded
    const fontFaceSet = {
        status: "loaded",
        size: 0,
        check: () => true,
        load: () => Promise.resolve([]),
        forEach() {},
        addEventListener() {},
        removeEventListener() {},
        dispatchEvent() {
            return true;
        },
    };
    fontFaceSet.ready = Promise.resolve(fontFaceSet);
    document.fonts = fontFaceSet;
