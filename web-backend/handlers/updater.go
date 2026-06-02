package handlers

import (
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"golang.org/x/mod/semver"
)

type GitHubRelease struct {
	TagName     string `json:"tag_name"`
	Body        string `json:"body"`
	PublishedAt string `json:"published_at"`
	Assets      []struct {
		Name               string `json:"name"`
		BrowserDownloadURL string `json:"browser_download_url"`
	} `json:"assets"`
}

type CachedRelease struct {
	Version   string
	Notes     string
	PubDate   string
	Platforms map[string]map[string]string // e.g. "windows-x86_64": {"url": "...", "signature": "..."}
	FetchTime time.Time
}

var (
	releaseCache *CachedRelease
	cacheMutex   sync.RWMutex
	cacheTTL     = 10 * time.Minute
)

func getGithubToken() string {
	return os.Getenv("GITHUB_TOKEN")
}

func getGithubRepo() string {
	repo := os.Getenv("GITHUB_REPO")
	if repo == "" {
		// Provide a fallback or log a warning
		repo = "DawnLandTeam/dawnland-launcher"
	}
	return repo
}

// fetchSignature reads the actual .sig file text content from the URL
func fetchSignature(url string) (string, error) {
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return "", err
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		return "", fmt.Errorf("failed to fetch signature: HTTP %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}

	// Signatures are usually base64 strings, trim any surrounding whitespace/newlines
	return strings.TrimSpace(string(body)), nil
}

func fetchLatestRelease() (*CachedRelease, error) {
	repo := getGithubRepo()
	apiURL := fmt.Sprintf("https://api.github.com/repos/%s/releases/latest", repo)

	req, err := http.NewRequest("GET", apiURL, nil)
	if err != nil {
		return nil, err
	}

	if token := getGithubToken(); token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	req.Header.Set("Accept", "application/vnd.github.v3+json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		if resp.StatusCode == 404 {
			// This means the repository doesn't have any releases yet, or the repository doesn't exist.
			// Return a special error that we can handle gracefully.
			return nil, fmt.Errorf("no releases found (HTTP 404)")
		}
		return nil, fmt.Errorf("github API error (HTTP %d): %s", resp.StatusCode, string(body))
	}

	var release GitHubRelease
	if err := json.NewDecoder(resp.Body).Decode(&release); err != nil {
		return nil, err
	}

	// Clean tag e.g. "v1.0.0" -> "1.0.0"
	version := strings.TrimPrefix(release.TagName, "v")

	cached := &CachedRelease{
		Version:   version,
		Notes:     release.Body,
		PubDate:   release.PublishedAt,
		Platforms: make(map[string]map[string]string),
		FetchTime: time.Now(),
	}

	// Map assets to URLs
	assetURLs := make(map[string]string)
	for _, asset := range release.Assets {
		assetURLs[asset.Name] = asset.BrowserDownloadURL
	}

	// Helper to find matching assets
	findAssets := func(suffix string) (url string, sigUrl string, found bool) {
		for name, dlUrl := range assetURLs {
			if strings.HasSuffix(name, suffix) {
				url = dlUrl
				if sUrl, ok := assetURLs[name+".sig"]; ok {
					sigUrl = sUrl
				}
				found = true
				return
			}
		}
		return
	}

	// Tauri v2 standard suffix mappings
	mappings := []struct {
		TauriPlatform string
		Suffix        string
	}{
		{"windows-x86_64", ".msi.zip"},
		{"darwin-x86_64", "x64.app.tar.gz"},
		{"darwin-aarch64", "aarch64.app.tar.gz"},
		{"linux-x86_64", "amd64.AppImage.tar.gz"},
	}

	for _, m := range mappings {
		if url, sigUrl, found := findAssets(m.Suffix); found && sigUrl != "" {
			sigContent, err := fetchSignature(sigUrl)
			if err != nil {
				log.Printf("[Updater] Failed to fetch signature for %s: %v", m.TauriPlatform, err)
				continue
			}
			cached.Platforms[m.TauriPlatform] = map[string]string{
				"url":       url,
				"signature": sigContent,
			}
		}
	}

	return cached, nil
}

func getCachedRelease() (*CachedRelease, error) {
	cacheMutex.RLock()
	if releaseCache != nil && time.Since(releaseCache.FetchTime) < cacheTTL {
		c := releaseCache
		cacheMutex.RUnlock()
		return c, nil
	}
	cacheMutex.RUnlock()

	cacheMutex.Lock()
	defer cacheMutex.Unlock()

	// Double check
	if releaseCache != nil && time.Since(releaseCache.FetchTime) < cacheTTL {
		return releaseCache, nil
	}

	release, err := fetchLatestRelease()
	if err != nil {
		// If fetch fails but we have a stale cache, return it rather than erroring out
		if releaseCache != nil {
			log.Printf("[Updater] Failed to fetch latest release, using stale cache: %v", err)
			return releaseCache, nil
		}
		return nil, err
	}

	releaseCache = release
	return releaseCache, nil
}

// CheckUpdate responds to Tauri updater plugin requests
func CheckUpdate(c *gin.Context) {
	target := c.Param("target")                  // e.g. windows-x86_64
	currentVersion := c.Param("current_version") // e.g. 0.1.0

	release, err := getCachedRelease()
	if err != nil {
		if strings.Contains(err.Error(), "no releases found") {
			log.Printf("[Updater] No releases found in repo. Treating as up to date.")
			c.Status(http.StatusNoContent)
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to fetch updates from GitHub: " + err.Error()})
		return
	}

	// Use semver to accurately compare versions
	vCurrent := "v" + strings.TrimPrefix(currentVersion, "v")
	vLatest := "v" + release.Version

	if !semver.IsValid(vCurrent) || !semver.IsValid(vLatest) {
		log.Printf("[Updater] Invalid version format. Current: %s, Latest: %s", vCurrent, vLatest)
		c.Status(http.StatusNoContent)
		return
	}

	// semver.Compare returns 1 if vLatest > vCurrent
	if semver.Compare(vLatest, vCurrent) > 0 {
		// Verify we have a matched asset for the requested target
		if _, ok := release.Platforms[target]; !ok {
			log.Printf("[Updater] Update available but no asset found for target OS/Arch: %s", target)
			c.Status(http.StatusNoContent)
			return
		}

		c.JSON(http.StatusOK, gin.H{
			"version":   release.Version,
			"notes":     release.Notes,
			"pub_date":  release.PubDate,
			"platforms": release.Platforms,
		})
		return
	}

	// No update available or current version is newer
	c.Status(http.StatusNoContent)
}
