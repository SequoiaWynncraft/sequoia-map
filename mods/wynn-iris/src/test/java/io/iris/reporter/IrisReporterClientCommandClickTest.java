package io.iris.reporter;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

public class IrisReporterClientCommandClickTest {
    @Test
    void modernRunCommandStripsLeadingSlash() {
        assertEquals("iris update apply", IrisReporterClient.normalizeRunCommandForModernClickEvent("/iris update apply"));
        assertEquals("iris update apply", IrisReporterClient.normalizeRunCommandForModernClickEvent("iris update apply"));
    }

    @Test
    void legacyRunCommandEnsuresLeadingSlash() {
        assertEquals("/iris update apply", IrisReporterClient.normalizeRunCommandForLegacyClickEvent("iris update apply"));
        assertEquals("/iris update apply", IrisReporterClient.normalizeRunCommandForLegacyClickEvent("/iris update apply"));
    }
}
