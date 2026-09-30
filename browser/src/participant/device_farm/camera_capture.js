// Device Farm's synthetic camera advertises up to 4K. Limit camera capture,
// including later constraint updates, before frontend media initialization.
((mediaData) => {
    const mediaDevices = navigator.mediaDevices;
    if (!mediaDevices) return;

    const captureConstraints = (constraints) => ({
        ...constraints,
        width: { ideal: 1280, max: 1280 },
        height: { ideal: 720, max: 720 },
    });
    const getUserMedia = mediaDevices.getUserMedia.bind(mediaDevices);
    let clip;
    const loadClip = async () => {
        const video = document.createElement("video");
        video.src = URL.createObjectURL(new Blob([
            Uint8Array.from(atob(mediaData), (character) => character.charCodeAt(0)),
        ], { type: "video/webm" }));
        video.loop = true;
        // Muting can silence captured audio-only clips in Chrome; volume only silences playback.
        video.volume = 0;
        await video.play();
        // ponytail: keep one decoder for the page; pause it if idle capture becomes costly.
        const stream = video.captureStream();
        if (!stream.getTracks().length) throw new Error("Selected media clip has no capture tracks");
        return stream;
    };
    mediaDevices.getUserMedia = async (constraints) => {
        const stream = await getUserMedia({
            ...constraints,
            video: constraints.video
                ? captureConstraints(constraints.video)
                : constraints.video,
        });
        if (mediaData) {
            try {
                clip ??= loadClip();
                const source = await clip;
                for (const original of stream.getTracks()) {
                    const selected = source.getTracks().find((track) => track.kind === original.kind);
                    if (!selected) continue;
                    const track = selected.clone();
                    const {deviceId, groupId} = original.getSettings();
                    const getSettings = track.getSettings.bind(track);
                    const getCapabilities = track.getCapabilities.bind(track);
                    Object.defineProperty(track, "label", {get: () => original.label});
                    track.getSettings = () => ({...getSettings(), deviceId, groupId});
                    track.getCapabilities = () => ({...getCapabilities(), deviceId, groupId});
                    original.stop();
                    stream.removeTrack(original);
                    stream.addTrack(track);
                }
            } catch (error) {
                stream.getTracks().forEach((track) => track.stop());
                throw error;
            }
        }
        for (const track of stream.getVideoTracks()) {
            const applyConstraints = track.applyConstraints.bind(track);
            track.applyConstraints = (constraints) => applyConstraints(captureConstraints(constraints));
        }
        return stream;
    };
})(/*MEDIA_DATA*/null);
