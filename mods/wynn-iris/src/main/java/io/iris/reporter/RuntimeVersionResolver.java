package io.iris.reporter;

import net.fabricmc.loader.api.FabricLoader;

public final class RuntimeVersionResolver {
    private static final String MOD_ID = "wynn_iris";
    private static final String MINECRAFT_MOD_ID = "minecraft";
    private static final String FALLBACK_MOD_VERSION = IrisReporterClient.MOD_VERSION;

    private RuntimeVersionResolver() {}

    public static String currentModVersion() {
        return FabricLoader.getInstance()
            .getModContainer(MOD_ID)
            .map(container -> container.getMetadata().getVersion().getFriendlyString())
            .filter(version -> version != null && !version.isBlank())
            .orElse(FALLBACK_MOD_VERSION);
    }

    public static String currentMinecraftVersion() {
        return FabricLoader.getInstance()
            .getModContainer(MINECRAFT_MOD_ID)
            .map(container -> container.getMetadata().getVersion().getFriendlyString())
            .filter(version -> version != null && !version.isBlank())
            .orElse("unknown");
    }
}
