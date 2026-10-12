kaplay({
    width: 320,
    height: 240,
    background: [16, 20, 28],
});

onLoad(() => {
    add([rect(320, 240), color(30, 40, 60)]);
    add([circle(24), pos(80, 160), color(240, 80, 80)]);
    add([text("hello native"), pos(20, 20), color(255, 220, 0)]);
    add([text("kaplay"), pos(20, 96), color(120, 220, 255)]);
});
