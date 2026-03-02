package io.iris.reporter;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

public class ReporterConfigDefaultsTest {
    @Test
    void defaultIngestBaseUrlTargetsSeqwawa() {
        ReporterConfig config = new ReporterConfig();
        assertEquals("https://map.seqwawa.com", config.ingestBaseUrl);
    }

    @Test
    void normalizeIngestBaseUrlInputStripsWildcardsAndTrailingSlashes() {
        assertEquals(
                "https://map.seqwawa.com",
                ReporterRuntime.normalizeIngestBaseUrlInput("https://map.seqwawa.com/*"));
        assertEquals(
                "https://map.seqwawa.com",
                ReporterRuntime.normalizeIngestBaseUrlInput(" https://map.seqwawa.com/ "));
        assertEquals(
                "https://map.seqwawa.com/api",
                ReporterRuntime.normalizeIngestBaseUrlInput("https://map.seqwawa.com/api/***///"));
    }
}
