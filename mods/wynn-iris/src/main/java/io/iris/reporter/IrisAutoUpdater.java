package io.iris.reporter;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

import java.io.IOException;
import java.net.URI;
import java.net.URISyntaxException;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.function.BiConsumer;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class IrisAutoUpdater {
    static final String DEFAULT_REPO = "OneNoted/sequoia-map";
    private static final Gson GSON = new GsonBuilder().create();
    private static final Set<String> ALLOWED_DOWNLOAD_HOSTS = Set.of(
        "github.com",
        "objects.githubusercontent.com",
        "github-releases.githubusercontent.com"
    );
    private static final Pattern REPO_PATTERN = Pattern.compile("^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$");
    private static final Pattern SEMVER_PATTERN =
        Pattern.compile("^(\\d+)\\.(\\d+)\\.(\\d+)(?:-([0-9A-Za-z.-]+))?(?:\\+([0-9A-Za-z._-]+))?$");

    private final HttpClient httpClient;
    private final ExecutorService requestExecutor;

    public IrisAutoUpdater() {
        this(
            HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(6)).build(),
            Executors.newSingleThreadExecutor(r -> {
                Thread thread = new Thread(r, "wynn-iris-updater");
                thread.setDaemon(true);
                return thread;
            })
        );
    }

    IrisAutoUpdater(HttpClient httpClient, ExecutorService requestExecutor) {
        this.httpClient = Objects.requireNonNull(httpClient, "httpClient");
        this.requestExecutor = Objects.requireNonNull(requestExecutor, "requestExecutor");
    }

    public CompletableFuture<CheckResult> checkForUpdateAsync(
        String repo,
        boolean includePrerelease,
        String currentVersion,
        String minecraftVersion
    ) {
        String normalizedRepo = normalizeRepo(repo);
        String repoError = repoValidationError(normalizedRepo);
        if (repoError != null) {
            return CompletableFuture.completedFuture(CheckResult.failed(repoError));
        }

        SemanticVersion current = parseSemanticVersion(currentVersion);
        if (current == null) {
            return CompletableFuture.completedFuture(CheckResult.failed("current_version_invalid"));
        }

        String normalizedMinecraftVersion = minecraftVersion == null ? "" : minecraftVersion.trim();
        return CompletableFuture.supplyAsync(
            () -> checkForUpdate(normalizedRepo, includePrerelease, current, normalizedMinecraftVersion),
            requestExecutor
        );
    }

    public CompletableFuture<ApplyResult> applyUpdateAsync(String assetUrl) {
        return CompletableFuture.supplyAsync(() -> applyUpdate(assetUrl), requestExecutor);
    }

    private CheckResult checkForUpdate(
        String repo,
        boolean includePrerelease,
        SemanticVersion currentVersion,
        String minecraftVersion
    ) {
        List<ReleaseSummary> releases;
        try {
            releases = fetchReleases(repo);
        } catch (UpdaterException e) {
            IrisReporterClient.LOGGER.warn("Iris update check failed: {}", e.reason());
            return CheckResult.failed(e.reason());
        }

        SelectionResult selection = selectLatestCompatibleRelease(
            releases,
            currentVersion,
            minecraftVersion,
            includePrerelease
        );

        if (selection.release() == null || selection.asset() == null || selection.version() == null) {
            if (selection.newerReleaseSeen()) {
                String latest = selection.latestSeenVersion() == null ? currentVersion.toString() : selection.latestSeenVersion().toString();
                return CheckResult.noCompatibleRelease(currentVersion.toString(), latest, "no_compatible_release_asset");
            }
            return CheckResult.upToDate(currentVersion.toString());
        }

        return CheckResult.updateAvailable(
            currentVersion.toString(),
            selection.version().toString(),
            selection.release().htmlUrl(),
            selection.asset().downloadUrl()
        );
    }

    private ApplyResult applyUpdate(String assetUrl) {
        String validationError = downloadUrlValidationError(assetUrl);
        if (validationError != null) {
            return ApplyResult.failed(validationError);
        }

        Path currentJar = resolveCurrentJarPath();
        if (currentJar == null) {
            return ApplyResult.failed("current_jar_path_unavailable");
        }

        URI uri = URI.create(assetUrl.trim());
        byte[] payload;
        try {
            payload = downloadJarPayload(uri);
        } catch (UpdaterException e) {
            IrisReporterClient.LOGGER.warn("Iris update download failed: {}", e.reason());
            return ApplyResult.failed(e.reason());
        }

        return installDownloadedJar(currentJar, payload);
    }

    private List<ReleaseSummary> fetchReleases(String repo) throws UpdaterException {
        URI endpoint = URI.create("https://api.github.com/repos/" + repo + "/releases?per_page=30");
        HttpRequest request = HttpRequest.newBuilder()
            .uri(endpoint)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "wynn-iris-updater")
            .timeout(Duration.ofSeconds(10))
            .GET()
            .build();

        HttpResponse<String> response;
        try {
            response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());
        } catch (IOException e) {
            throw new UpdaterException("release_fetch_io", e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new UpdaterException("release_fetch_interrupted", e);
        }

        if (response.statusCode() / 100 != 2) {
            throw new UpdaterException("release_fetch_http_" + response.statusCode());
        }

        JsonElement root;
        try {
            root = GSON.fromJson(response.body(), JsonElement.class);
        } catch (RuntimeException e) {
            throw new UpdaterException("release_json_invalid", e);
        }

        if (root == null || !root.isJsonArray()) {
            throw new UpdaterException("release_json_not_array");
        }

        JsonArray array = root.getAsJsonArray();
        List<ReleaseSummary> releases = new ArrayList<>();
        for (JsonElement element : array) {
            if (element == null || !element.isJsonObject()) {
                continue;
            }
            JsonObject object = element.getAsJsonObject();
            String tagName = getString(object, "tag_name");
            String htmlUrl = getString(object, "html_url");
            boolean prerelease = getBoolean(object, "prerelease", false);

            List<ReleaseAsset> assets = new ArrayList<>();
            JsonArray assetsArray = getArray(object, "assets");
            if (assetsArray != null) {
                for (JsonElement assetElement : assetsArray) {
                    if (assetElement == null || !assetElement.isJsonObject()) {
                        continue;
                    }
                    JsonObject assetObject = assetElement.getAsJsonObject();
                    String name = getString(assetObject, "name");
                    String downloadUrl = getString(assetObject, "browser_download_url");
                    if (name == null || name.isBlank() || downloadUrl == null || downloadUrl.isBlank()) {
                        continue;
                    }
                    assets.add(new ReleaseAsset(name, downloadUrl));
                }
            }

            releases.add(new ReleaseSummary(tagName, prerelease, htmlUrl, assets));
        }

        return releases;
    }

    private byte[] downloadJarPayload(URI originalUri) throws UpdaterException {
        URI current = originalUri;
        for (int redirectCount = 0; redirectCount <= 5; redirectCount++) {
            String uriError = downloadUriValidationError(current);
            if (uriError != null) {
                throw new UpdaterException(uriError);
            }

            HttpRequest request = HttpRequest.newBuilder()
                .uri(current)
                .header("User-Agent", "wynn-iris-updater")
                .timeout(Duration.ofSeconds(20))
                .GET()
                .build();

            HttpResponse<byte[]> response;
            try {
                response = httpClient.send(request, HttpResponse.BodyHandlers.ofByteArray());
            } catch (IOException e) {
                throw new UpdaterException("update_download_io", e);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new UpdaterException("update_download_interrupted", e);
            }

            int status = response.statusCode();
            if (isRedirectStatus(status)) {
                String location = response.headers().firstValue("location").orElse(null);
                if (location == null || location.isBlank()) {
                    throw new UpdaterException("update_download_redirect_missing_location");
                }
                try {
                    current = resolveRedirectUri(current, location);
                } catch (UpdaterException e) {
                    throw e;
                }
                continue;
            }

            if (status / 100 != 2) {
                throw new UpdaterException("update_download_http_" + status);
            }

            byte[] body = response.body();
            if (body == null || body.length == 0) {
                throw new UpdaterException("update_download_empty");
            }
            return body;
        }

        throw new UpdaterException("update_download_too_many_redirects");
    }

    static SelectionResult selectLatestCompatibleRelease(
        List<ReleaseSummary> releases,
        SemanticVersion currentVersion,
        String minecraftVersion,
        boolean includePrerelease
    ) {
        SemanticVersion latestSeenVersion = null;
        SemanticVersion bestCompatibleVersion = null;
        ReleaseSummary bestRelease = null;
        ReleaseAsset bestAsset = null;
        boolean newerReleaseSeen = false;

        String marker = "mc" + (minecraftVersion == null ? "" : minecraftVersion.trim());
        for (ReleaseSummary release : releases) {
            if (release == null) {
                continue;
            }
            if (release.prerelease() && !includePrerelease) {
                continue;
            }

            SemanticVersion releaseVersion = parseSemanticVersion(release.tagName());
            if (releaseVersion == null || releaseVersion.compareTo(currentVersion) <= 0) {
                continue;
            }

            newerReleaseSeen = true;
            if (latestSeenVersion == null || releaseVersion.compareTo(latestSeenVersion) > 0) {
                latestSeenVersion = releaseVersion;
            }

            ReleaseAsset matched = findCompatibleAsset(release.assets(), marker);
            if (matched == null) {
                continue;
            }

            if (bestCompatibleVersion == null || releaseVersion.compareTo(bestCompatibleVersion) > 0) {
                bestCompatibleVersion = releaseVersion;
                bestRelease = release;
                bestAsset = matched;
            }
        }

        return new SelectionResult(bestRelease, bestAsset, bestCompatibleVersion, latestSeenVersion, newerReleaseSeen);
    }

    private static ReleaseAsset findCompatibleAsset(List<ReleaseAsset> assets, String minecraftMarker) {
        if (assets == null || assets.isEmpty()) {
            return null;
        }

        String normalizedMarker = minecraftMarker == null ? "" : minecraftMarker.trim().toLowerCase(Locale.ROOT);
        for (ReleaseAsset asset : assets) {
            if (asset == null) {
                continue;
            }
            String name = asset.name();
            if (name == null) {
                continue;
            }
            String normalized = name.toLowerCase(Locale.ROOT);
            if (!normalized.endsWith(".jar")) {
                continue;
            }
            if (normalized.endsWith("-sources.jar")) {
                continue;
            }
            if (normalizedMarker.isEmpty() || !normalized.contains(normalizedMarker)) {
                continue;
            }
            return asset;
        }
        return null;
    }

    public static String normalizeRepo(String repo) {
        if (repo == null || repo.isBlank()) {
            return DEFAULT_REPO;
        }
        return repo.trim();
    }

    public static String repoValidationError(String repo) {
        String normalized = normalizeRepo(repo);
        if (normalized.isBlank()) {
            return "update_repo_missing";
        }
        if (!REPO_PATTERN.matcher(normalized).matches()) {
            return "update_repo_invalid";
        }
        return null;
    }

    public static String downloadUrlValidationError(String downloadUrl) {
        if (downloadUrl == null || downloadUrl.isBlank()) {
            return "update_asset_url_missing";
        }

        URI uri;
        try {
            uri = URI.create(downloadUrl.trim());
        } catch (IllegalArgumentException e) {
            return "update_asset_url_invalid";
        }

        return downloadUriValidationError(uri);
    }

    static String downloadUriValidationError(URI uri) {
        if (uri == null) {
            return "update_asset_url_invalid";
        }

        String scheme = uri.getScheme();
        if (scheme == null || !scheme.equalsIgnoreCase("https")) {
            return "update_asset_url_insecure";
        }

        String host = uri.getHost();
        if (host == null || host.isBlank()) {
            return "update_asset_url_missing_host";
        }

        String normalizedHost = host.toLowerCase(Locale.ROOT);
        if (!ALLOWED_DOWNLOAD_HOSTS.contains(normalizedHost)) {
            return "update_asset_host_not_allowed";
        }

        return null;
    }

    static SemanticVersion parseSemanticVersion(String rawVersion) {
        if (rawVersion == null || rawVersion.isBlank()) {
            return null;
        }

        String normalized = rawVersion.trim();
        String lower = normalized.toLowerCase(Locale.ROOT);
        if (lower.startsWith("iris-v")) {
            normalized = normalized.substring(6);
        } else if (lower.startsWith("iris-")) {
            normalized = normalized.substring(5);
        }
        if (normalized.startsWith("v") || normalized.startsWith("V")) {
            normalized = normalized.substring(1);
        }

        Matcher matcher = SEMVER_PATTERN.matcher(normalized);
        if (!matcher.matches()) {
            return null;
        }

        int major;
        int minor;
        int patch;
        try {
            major = Integer.parseInt(matcher.group(1));
            minor = Integer.parseInt(matcher.group(2));
            patch = Integer.parseInt(matcher.group(3));
        } catch (NumberFormatException e) {
            return null;
        }

        String prerelease = matcher.group(4);
        String buildMetadata = matcher.group(5);
        List<String> prereleaseIdentifiers = parsePrereleaseIdentifiers(prerelease);

        return new SemanticVersion(major, minor, patch, prereleaseIdentifiers, buildMetadata);
    }

    private static List<String> parsePrereleaseIdentifiers(String prerelease) {
        List<String> identifiers = new ArrayList<>();
        if (prerelease == null || prerelease.isBlank()) {
            return identifiers;
        }

        String[] parts = prerelease.split("\\.");
        for (String part : parts) {
            if (part == null || part.isBlank()) {
                return List.of();
            }
            identifiers.add(part);
        }
        return identifiers;
    }

    static Path resolveCurrentJarPath() {
        try {
            if (IrisReporterClient.class.getProtectionDomain() == null
                || IrisReporterClient.class.getProtectionDomain().getCodeSource() == null
                || IrisReporterClient.class.getProtectionDomain().getCodeSource().getLocation() == null) {
                return null;
            }
            URI location = IrisReporterClient.class.getProtectionDomain().getCodeSource().getLocation().toURI();
            return resolveCurrentJarPathFromLocation(location);
        } catch (URISyntaxException e) {
            return null;
        }
    }

    static Path resolveCurrentJarPathFromLocation(URI location) {
        if (location == null) {
            return null;
        }

        Path path;
        try {
            path = Paths.get(location).normalize();
        } catch (Exception e) {
            return null;
        }

        if (Files.isDirectory(path)) {
            return null;
        }
        Path fileName = path.getFileName();
        if (fileName == null) {
            return null;
        }
        String name = fileName.toString().toLowerCase(Locale.ROOT);
        if (!name.endsWith(".jar")) {
            return null;
        }
        return path;
    }

    static ApplyResult installDownloadedJar(Path currentJarPath, byte[] jarPayload) {
        return installDownloadedJar(currentJarPath, jarPayload, IrisAutoUpdater::replaceWithMove);
    }

    static ApplyResult installDownloadedJar(
        Path currentJarPath,
        byte[] jarPayload,
        BiConsumer<Path, Path> replacer
    ) {
        if (currentJarPath == null) {
            return ApplyResult.failed("current_jar_path_unavailable");
        }
        if (jarPayload == null || jarPayload.length == 0) {
            return ApplyResult.failed("update_payload_empty");
        }
        if (!Files.exists(currentJarPath)) {
            return ApplyResult.failed("current_jar_missing");
        }

        Path parent = currentJarPath.getParent();
        if (parent == null) {
            return ApplyResult.failed("current_jar_parent_missing");
        }

        String fileName = currentJarPath.getFileName().toString();
        Path tempPath = parent.resolve(fileName + ".download.tmp");
        Path backupPath = parent.resolve(fileName + ".bak");
        boolean backupCreated = false;

        try {
            Files.createDirectories(parent);
            Files.write(tempPath, jarPayload);

            Files.copy(currentJarPath, backupPath, StandardCopyOption.REPLACE_EXISTING);
            backupCreated = true;

            replacer.accept(tempPath, currentJarPath);

            return ApplyResult.applied(currentJarPath.toString(), backupPath.toString());
        } catch (IOException e) {
            if (backupCreated) {
                try {
                    Files.copy(backupPath, currentJarPath, StandardCopyOption.REPLACE_EXISTING);
                } catch (IOException restoreError) {
                    IrisReporterClient.LOGGER.warn("Iris updater restore failed", restoreError);
                }
            }
            return ApplyResult.failed("update_install_failed");
        } catch (RuntimeException e) {
            if (backupCreated) {
                try {
                    Files.copy(backupPath, currentJarPath, StandardCopyOption.REPLACE_EXISTING);
                } catch (IOException restoreError) {
                    IrisReporterClient.LOGGER.warn("Iris updater restore failed", restoreError);
                }
            }
            return ApplyResult.failed("update_install_failed");
        } finally {
            try {
                Files.deleteIfExists(tempPath);
            } catch (IOException ignored) {
                // Best effort cleanup only.
            }
        }
    }

    private static void replaceWithMove(Path source, Path target) {
        try {
            try {
                Files.move(source, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
    }

    private static boolean isRedirectStatus(int statusCode) {
        return statusCode == 301 || statusCode == 302 || statusCode == 303 || statusCode == 307 || statusCode == 308;
    }

    private static URI resolveRedirectUri(URI current, String location) throws UpdaterException {
        try {
            URI redirect = URI.create(location);
            if (redirect.isAbsolute()) {
                return redirect;
            }
            return current.resolve(redirect);
        } catch (IllegalArgumentException e) {
            throw new UpdaterException("update_download_redirect_invalid", e);
        }
    }

    private static String getString(JsonObject object, String key) {
        if (object == null || key == null || !object.has(key)) {
            return null;
        }
        JsonElement value = object.get(key);
        if (value == null || value.isJsonNull()) {
            return null;
        }
        try {
            return value.getAsString();
        } catch (RuntimeException e) {
            return null;
        }
    }

    private static boolean getBoolean(JsonObject object, String key, boolean fallback) {
        if (object == null || key == null || !object.has(key)) {
            return fallback;
        }
        JsonElement value = object.get(key);
        if (value == null || value.isJsonNull()) {
            return fallback;
        }
        try {
            return value.getAsBoolean();
        } catch (RuntimeException e) {
            return fallback;
        }
    }

    private static JsonArray getArray(JsonObject object, String key) {
        if (object == null || key == null || !object.has(key)) {
            return null;
        }
        JsonElement value = object.get(key);
        if (value == null || !value.isJsonArray()) {
            return null;
        }
        return value.getAsJsonArray();
    }

    public enum CheckStatus {
        UP_TO_DATE,
        UPDATE_AVAILABLE,
        NO_COMPATIBLE_RELEASE,
        FAILED
    }

    public record CheckResult(
        CheckStatus status,
        String currentVersion,
        String latestVersion,
        String releaseUrl,
        String assetUrl,
        String reason
    ) {
        static CheckResult upToDate(String currentVersion) {
            return new CheckResult(CheckStatus.UP_TO_DATE, currentVersion, currentVersion, null, null, "up_to_date");
        }

        static CheckResult updateAvailable(
            String currentVersion,
            String latestVersion,
            String releaseUrl,
            String assetUrl
        ) {
            return new CheckResult(
                CheckStatus.UPDATE_AVAILABLE,
                currentVersion,
                latestVersion,
                releaseUrl,
                assetUrl,
                "update_available"
            );
        }

        static CheckResult noCompatibleRelease(String currentVersion, String latestVersion, String reason) {
            return new CheckResult(CheckStatus.NO_COMPATIBLE_RELEASE, currentVersion, latestVersion, null, null, reason);
        }

        static CheckResult failed(String reason) {
            return new CheckResult(CheckStatus.FAILED, null, null, null, null, reason);
        }
    }

    public enum ApplyStatus {
        APPLIED,
        FAILED
    }

    public record ApplyResult(
        ApplyStatus status,
        String installedPath,
        String backupPath,
        String reason
    ) {
        static ApplyResult applied(String installedPath, String backupPath) {
            return new ApplyResult(ApplyStatus.APPLIED, installedPath, backupPath, "applied");
        }

        static ApplyResult failed(String reason) {
            return new ApplyResult(ApplyStatus.FAILED, null, null, reason == null || reason.isBlank() ? "failed" : reason);
        }
    }

    static record ReleaseSummary(String tagName, boolean prerelease, String htmlUrl, List<ReleaseAsset> assets) {}

    static record ReleaseAsset(String name, String downloadUrl) {}

    static record SelectionResult(
        ReleaseSummary release,
        ReleaseAsset asset,
        SemanticVersion version,
        SemanticVersion latestSeenVersion,
        boolean newerReleaseSeen
    ) {}

    static final class SemanticVersion implements Comparable<SemanticVersion> {
        private final int major;
        private final int minor;
        private final int patch;
        private final List<String> prerelease;
        private final String buildMetadata;

        private SemanticVersion(int major, int minor, int patch, List<String> prerelease, String buildMetadata) {
            this.major = major;
            this.minor = minor;
            this.patch = patch;
            this.prerelease = List.copyOf(prerelease == null ? List.of() : prerelease);
            this.buildMetadata = buildMetadata == null ? "" : buildMetadata;
        }

        @Override
        public int compareTo(SemanticVersion other) {
            if (other == null) {
                return 1;
            }

            int cmp = Integer.compare(major, other.major);
            if (cmp != 0) {
                return cmp;
            }
            cmp = Integer.compare(minor, other.minor);
            if (cmp != 0) {
                return cmp;
            }
            cmp = Integer.compare(patch, other.patch);
            if (cmp != 0) {
                return cmp;
            }

            boolean thisHasPrerelease = !prerelease.isEmpty();
            boolean otherHasPrerelease = !other.prerelease.isEmpty();
            if (!thisHasPrerelease && !otherHasPrerelease) {
                return 0;
            }
            if (!thisHasPrerelease) {
                return 1;
            }
            if (!otherHasPrerelease) {
                return -1;
            }

            int len = Math.max(prerelease.size(), other.prerelease.size());
            for (int idx = 0; idx < len; idx++) {
                if (idx >= prerelease.size()) {
                    return -1;
                }
                if (idx >= other.prerelease.size()) {
                    return 1;
                }

                String left = prerelease.get(idx);
                String right = other.prerelease.get(idx);
                boolean leftNumeric = isNumeric(left);
                boolean rightNumeric = isNumeric(right);

                if (leftNumeric && rightNumeric) {
                    cmp = Integer.compare(Integer.parseInt(left), Integer.parseInt(right));
                } else if (leftNumeric) {
                    cmp = -1;
                } else if (rightNumeric) {
                    cmp = 1;
                } else {
                    cmp = left.compareTo(right);
                }

                if (cmp != 0) {
                    return cmp;
                }
            }

            return 0;
        }

        private static boolean isNumeric(String value) {
            if (value == null || value.isEmpty()) {
                return false;
            }
            for (int idx = 0; idx < value.length(); idx++) {
                if (!Character.isDigit(value.charAt(idx))) {
                    return false;
                }
            }
            return true;
        }

        @Override
        public String toString() {
            StringBuilder out = new StringBuilder();
            out.append(major).append('.').append(minor).append('.').append(patch);
            if (!prerelease.isEmpty()) {
                out.append('-').append(String.join(".", prerelease));
            }
            if (!buildMetadata.isBlank()) {
                out.append('+').append(buildMetadata);
            }
            return out.toString();
        }
    }

    private static final class UpdaterException extends Exception {
        private final String reason;

        private UpdaterException(String reason) {
            super(reason);
            this.reason = reason;
        }

        private UpdaterException(String reason, Throwable cause) {
            super(reason, cause);
            this.reason = reason;
        }

        String reason() {
            return reason;
        }
    }
}
