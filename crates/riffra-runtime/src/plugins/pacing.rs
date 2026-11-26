//! Plugin classes that synthesize in wall-clock time.
//!
//! The VST3 specification gives a plugin no way to say that its audio is produced by a
//! background pipeline, so Riffra keeps this knowledge itself. A render that loads one of these
//! classes runs at 1.0x speed and processes the plugin for its warm-up before the first sample is
//! written; a faster render drops the audio that is not ready yet.

/// A plugin class that needs real-time rendering.
pub(crate) struct RealtimeClass {
    /// File stem of the plugin's VST3 bundle, also its name in render results and errors.
    pub(crate) name: &'static str,
    /// Wall-clock seconds the plugin is processed before the render starts.
    pub(crate) warm_up_seconds: f64,
}

const REALTIME_CLASSES: [RealtimeClass; 2] = [
    RealtimeClass {
        name: "Synthesizer V Studio 2 Plugin",
        warm_up_seconds: 30.0,
    },
    RealtimeClass {
        name: "VoiSona Song",
        warm_up_seconds: 20.0,
    },
];

/// Returns the real-time class of the plugin at `path`, if it is a known one.
pub(crate) fn realtime_class(path: &str) -> Option<&'static RealtimeClass> {
    let bundle = super::display_name(std::path::Path::new(path));
    REALTIME_CLASSES
        .iter()
        .find(|class| class.name.eq_ignore_ascii_case(&bundle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_classes_match_by_bundle_name() {
        // Arrange
        let synthv = "C:/Program Files/Common Files/VST3/Synthesizer V Studio 2 Plugin.vst3";
        let voisona = "C:/Program Files/Common Files/VST3/Techno-Speech/VoiSona Song.vst3";
        let other = "C:/Program Files/Common Files/VST3/Pianoverse.vst3";

        // Act / Assert
        assert_eq!(realtime_class(synthv).unwrap().warm_up_seconds, 30.0);
        assert_eq!(realtime_class(voisona).unwrap().name, "VoiSona Song");
        assert!(realtime_class(other).is_none());
    }
}
