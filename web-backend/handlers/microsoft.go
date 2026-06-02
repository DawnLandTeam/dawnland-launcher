package handlers

import (
	"encoding/json"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"strings"

	"github.com/gin-gonic/gin"
)

const (
	msScope = "XboxLive.signin offline_access"
)

func getMsClientConfig() (string, string) {
	clientID := os.Getenv("MS_CLIENT_ID")
	if clientID == "" {
		clientID = "780ab3ca-a1a0-4830-ac98-92a595e85a13" // Fallback
	}
	tenantID := os.Getenv("MS_TENANT_ID")
	if tenantID == "" {
		tenantID = "consumers" // Fallback
	}
	return clientID, tenantID
}

// DeviceCodeResponse represents the device code flow response from Microsoft
type DeviceCodeResponse struct {
	UserCode        string `json:"user_code"`
	DeviceCode      string `json:"device_code"`
	VerificationURI string `json:"verification_uri"`
	ExpiresIn       int64  `json:"expires_in"`
	Interval        *int64 `json:"interval,omitempty"`
	Message         *string `json:"message,omitempty"`
}

// MsDeviceCode handles the start of the device code flow
func MsDeviceCode(c *gin.Context) {
	clientID, tenantID := getMsClientConfig()
	
	targetURL := "https://login.microsoftonline.com/" + tenantID + "/oauth2/v2.0/devicecode"

	data := url.Values{}
	data.Set("client_id", clientID)
	data.Set("scope", msScope)

	req, err := http.NewRequest("POST", targetURL, strings.NewReader(data.Encode()))
	if err != nil {
		log.Printf("Error creating request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create request"})
		return
	}
	req.Header.Add("Content-Type", "application/x-www-form-urlencoded")

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		log.Printf("Error sending request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to proxy devicecode request"})
		return
	}
	defer resp.Body.Close()

	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to read response body"})
		return
	}

	if resp.StatusCode != http.StatusOK {
		c.Data(resp.StatusCode, resp.Header.Get("Content-Type"), bodyBytes)
		return
	}

	var msResp DeviceCodeResponse
	if err := json.Unmarshal(bodyBytes, &msResp); err != nil {
		log.Printf("Failed to decode response: %v", err)
		c.Data(resp.StatusCode, resp.Header.Get("Content-Type"), bodyBytes)
		return
	}

	c.JSON(http.StatusOK, msResp)
}

type TokenRequest struct {
	DeviceCode string `json:"device_code"`
}

// MsToken handles polling for the token
func MsToken(c *gin.Context) {
	clientID, tenantID := getMsClientConfig()
	
	var reqBody TokenRequest
	if err := c.ShouldBindJSON(&reqBody); err != nil {
		// Try form urlencoded as fallback if JSON fails
		reqBody.DeviceCode = c.PostForm("device_code")
		if reqBody.DeviceCode == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "Missing device_code"})
			return
		}
	}

	targetURL := "https://login.microsoftonline.com/" + tenantID + "/oauth2/v2.0/token"

	data := url.Values{}
	data.Set("client_id", clientID)
	data.Set("grant_type", "urn:ietf:params:oauth:grant-type:device_code")
	data.Set("device_code", reqBody.DeviceCode)

	req, err := http.NewRequest("POST", targetURL, strings.NewReader(data.Encode()))
	if err != nil {
		log.Printf("Error creating request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create request"})
		return
	}
	req.Header.Add("Content-Type", "application/x-www-form-urlencoded")

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		log.Printf("Error sending request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to proxy token request"})
		return
	}
	defer resp.Body.Close()

	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to read response body"})
		return
	}

	c.Data(resp.StatusCode, resp.Header.Get("Content-Type"), bodyBytes)
}

type RefreshRequest struct {
	RefreshToken string `json:"refresh_token"`
}

// MsRefresh handles token refresh
func MsRefresh(c *gin.Context) {
	clientID, tenantID := getMsClientConfig()
	
	var reqBody RefreshRequest
	if err := c.ShouldBindJSON(&reqBody); err != nil {
		reqBody.RefreshToken = c.PostForm("refresh_token")
		if reqBody.RefreshToken == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "Missing refresh_token"})
			return
		}
	}

	targetURL := "https://login.microsoftonline.com/" + tenantID + "/oauth2/v2.0/token"

	data := url.Values{}
	data.Set("client_id", clientID)
	data.Set("grant_type", "refresh_token")
	data.Set("refresh_token", reqBody.RefreshToken)

	req, err := http.NewRequest("POST", targetURL, strings.NewReader(data.Encode()))
	if err != nil {
		log.Printf("Error creating request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create request"})
		return
	}
	req.Header.Add("Content-Type", "application/x-www-form-urlencoded")

	client := &http.Client{}
	resp, err := client.Do(req)
	if err != nil {
		log.Printf("Error sending request: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to proxy refresh request"})
		return
	}
	defer resp.Body.Close()

	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to read response body"})
		return
	}

	// Just pass through the response directly
	c.Data(resp.StatusCode, resp.Header.Get("Content-Type"), bodyBytes)
}
