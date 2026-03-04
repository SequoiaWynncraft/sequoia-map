package io.iris.reporter;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

public class IrisAutoUpdaterReleaseSelectionTest {
    @Test
    void ignoresPrereleaseByDefaultAndUsesNewestStableCompatibleAsset() {
        IrisAutoUpdater.SemanticVersion current = IrisAutoUpdater.parseSemanticVersion("0.1.0");
        IrisAutoUpdater.SelectionResult result = IrisAutoUpdater.selectLatestCompatibleRelease(
            List.of(
                release("iris-v0.2.0-rc.1", true, asset("wynn-iris-mc1.21.11-0.2.0-rc.1.jar")),
                release("iris-v0.1.1", false, asset("wynn-iris-mc1.21.11-0.1.1.jar"))
            ),
            current,
            "1.21.11",
            false
        );

        assertNotNull(result.release());
        assertNotNull(result.version());
        assertEquals("0.1.1", result.version().toString());
        assertEquals("iris-v0.1.1", result.release().tagName());
    }

    @Test
    void ignoresSourcesJarAndMarksNoCompatibleAssetWhenOnlySourcesExist() {
        IrisAutoUpdater.SemanticVersion current = IrisAutoUpdater.parseSemanticVersion("0.1.0");
        IrisAutoUpdater.SelectionResult result = IrisAutoUpdater.selectLatestCompatibleRelease(
            List.of(
                release("iris-v0.1.1", false, asset("wynn-iris-mc1.21.11-0.1.1-sources.jar"))
            ),
            current,
            "1.21.11",
            false
        );

        assertNull(result.release());
        assertNull(result.asset());
        assertTrue(result.newerReleaseSeen());
    }

    @Test
    void selectsAssetMatchingCurrentMinecraftVersion() {
        IrisAutoUpdater.SemanticVersion current = IrisAutoUpdater.parseSemanticVersion("0.1.0");
        IrisAutoUpdater.SelectionResult result = IrisAutoUpdater.selectLatestCompatibleRelease(
            List.of(
                release(
                    "iris-v0.1.2",
                    false,
                    asset("wynn-iris-mc1.21.4-0.1.2.jar"),
                    asset("wynn-iris-mc1.21.11-0.1.2.jar")
                )
            ),
            current,
            "1.21.11",
            false
        );

        assertNotNull(result.asset());
        assertEquals("wynn-iris-mc1.21.11-0.1.2.jar", result.asset().name());
    }

    @Test
    void reportsNoNewerReleaseWhenCurrentVersionIsLatest() {
        IrisAutoUpdater.SemanticVersion current = IrisAutoUpdater.parseSemanticVersion("0.1.2");
        IrisAutoUpdater.SelectionResult result = IrisAutoUpdater.selectLatestCompatibleRelease(
            List.of(
                release("iris-v0.1.2", false, asset("wynn-iris-mc1.21.11-0.1.2.jar")),
                release("iris-v0.1.1", false, asset("wynn-iris-mc1.21.11-0.1.1.jar"))
            ),
            current,
            "1.21.11",
            false
        );

        assertNull(result.release());
        assertTrue(!result.newerReleaseSeen());
    }

    private static IrisAutoUpdater.ReleaseSummary release(
        String tag,
        boolean prerelease,
        IrisAutoUpdater.ReleaseAsset... assets
    ) {
        return new IrisAutoUpdater.ReleaseSummary(tag, prerelease, "https://github.com/OneNoted/sequoia-map/releases/tag/" + tag, List.of(assets));
    }

    private static IrisAutoUpdater.ReleaseAsset asset(String name) {
        return new IrisAutoUpdater.ReleaseAsset(name, "https://github.com/OneNoted/sequoia-map/releases/download/test/" + name);
    }
}
