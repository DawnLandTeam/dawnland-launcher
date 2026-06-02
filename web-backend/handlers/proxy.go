package handlers

import (
	"io"
	"log"
	"net/http"
	"os"

	"github.com/gin-gonic/gin"
)

// CurseForgeProxy forwards requests to the CurseForge API, injecting the
// server-side API key so that the client never sees it.
func CurseForgeProxy(c *gin.Context) {
	// path includes the full path like /mods/search or /mods/123/files
	// We need to extract just /mods/... part and preserve query string
	fullPath := c.Param("path")

	// Remove leading slash and keep the path
	pathPart := fullPath
	if len(pathPart) > 0 && pathPart[0] == '/' {
		pathPart = pathPart[1:]
	}

	// Extract query string from the original request
	queryString := c.Request.URL.RawQuery

	// Build target URL - path already includes /mods/...
	targetURL := "https://api.curseforge.com/v1/" + pathPart
	if queryString != "" {
		targetURL += "?" + queryString
	}

	log.Printf("Proxying request: %s %s -> %s", c.Request.Method, fullPath, targetURL)

	req, err := http.NewRequest(c.Request.Method, targetURL, c.Request.Body)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create proxy request"})
		return
	}

	// Inject the API key from the backend environment.
	apiKey := os.Getenv("CURSEFORGE_API_KEY")
	if apiKey == "" {
		log.Println("WARNING: CURSEFORGE_API_KEY is not set in .env")
	} else {
		log.Printf("API key loaded: %s...", apiKey[:min(10, len(apiKey))])
	}

	req.Header.Set("x-api-key", apiKey)
	req.Header.Set("User-Agent", "Dawnland-Backend/1.0")
	req.Header.Set("Accept", "application/json")

	log.Printf("Request headers: x-api-key=%s, User-Agent=%s", apiKey[:min(10, len(apiKey))], "Dawnland-Backend/1.0")

	// Copy over relevant content-type from the original request.
	if ct := c.GetHeader("Content-Type"); ct != "" {
		req.Header.Set("Content-Type", ct)
	}

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": "Failed to reach CurseForge API"})
		return
	}
	defer resp.Body.Close()

	// Stream the response body back to the client.
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to read upstream response"})
		return
	}

	// Forward the status code and content type from the upstream response.
	contentType := resp.Header.Get("Content-Type")
	if contentType == "" {
		contentType = "application/json"
	}
	c.Data(resp.StatusCode, contentType, body)
}
