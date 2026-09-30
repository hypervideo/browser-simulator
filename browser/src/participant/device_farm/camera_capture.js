// Device Farm's synthetic camera advertises up to 4K. Limit camera capture,
// including later constraint updates, before frontend media initialization.
(() => {
    const mediaDevices = navigator.mediaDevices;
    if (!mediaDevices) return;

    const captureConstraints = (constraints) => ({
        ...constraints,
        width: { ideal: 1280, max: 1280 },
        height: { ideal: 720, max: 720 },
    });
    const getUserMedia = mediaDevices.getUserMedia.bind(mediaDevices);
    mediaDevices.getUserMedia = async (constraints) => {
        const stream = await getUserMedia({
            ...constraints,
            video: constraints.video
                ? captureConstraints(constraints.video)
                : constraints.video,
        });
        for (const track of stream.getVideoTracks()) {
            const applyConstraints = track.applyConstraints.bind(track);
            track.applyConstraints = (constraints) => applyConstraints(captureConstraints(constraints));
        }
        return stream;
    };
})();
