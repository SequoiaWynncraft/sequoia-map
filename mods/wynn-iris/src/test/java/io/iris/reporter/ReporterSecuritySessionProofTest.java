package io.iris.reporter;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

public class ReporterSecuritySessionProofTest {
    @Test
    void parseAccessTokenFromSessionIdExtractsToken() {
        assertEquals("abc123", ReporterSecurity.parseAccessTokenFromSessionId("token:abc123:00000000000000000000000000000000"));
    }

    @Test
    void parseAccessTokenFromSessionIdRejectsInvalidValues() {
        assertNull(ReporterSecurity.parseAccessTokenFromSessionId(null));
        assertNull(ReporterSecurity.parseAccessTokenFromSessionId(""));
        assertNull(ReporterSecurity.parseAccessTokenFromSessionId("token::00000000000000000000000000000000"));
        assertNull(ReporterSecurity.parseAccessTokenFromSessionId("legacy-session-id"));
    }
}
