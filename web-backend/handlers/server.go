package handlers

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"

	"web-backend/database"
	"web-backend/models"

	"github.com/gin-gonic/gin"
)

// GetServers returns all approved (active) server entries with pagination and filtering.
func GetServers(c *gin.Context) {
	// Pagination parameters
	page, _ := strconv.Atoi(c.DefaultQuery("page", "1"))
	pageSize, _ := strconv.Atoi(c.DefaultQuery("pageSize", "20"))
	if page < 1 {
		page = 1
	}
	if pageSize < 1 || pageSize > 100 {
		pageSize = 20
	}
	offset := (page - 1) * pageSize

	// Filter parameters
	search := c.Query("search")
	mcVersion := c.Query("version")
	serverType := c.Query("serverType")
	authType := c.Query("authType")

	// Build query
	query := database.DB.Model(&models.Server{}).Where("is_active = ?", true)

	// Apply search filter (name, ip, motd)
	if search != "" {
		searchPattern := "%" + search + "%"
		query = query.Where("name LIKE ? OR ip LIKE ? OR motd LIKE ?", searchPattern, searchPattern, searchPattern)
	}

	// Apply version filter
	if mcVersion != "" {
		query = query.Where("version = ?", mcVersion)
	}

	// Apply server type filter
	if serverType != "" {
		query = query.Where("server_type = ?", serverType)
	}

	// Apply auth type filter
	if authType != "" {
		query = query.Where("auth_type = ?", authType)
	}

	// Count total
	var total int64
	query.Count(&total)

	// Fetch paginated results
	var servers []models.Server
	result := query.Order("is_online DESC, id DESC").Offset(offset).Limit(pageSize).Find(&servers)
	if result.Error != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to fetch servers"})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"data":       servers,
		"total":      total,
		"page":       page,
		"pageSize":   pageSize,
		"totalPages": (total + int64(pageSize) - 1) / int64(pageSize),
	})
}

// GetRecommendedServers returns active and online servers sorted strictly by heat DESC.
func GetRecommendedServers(c *gin.Context) {
	var servers []models.Server
	result := database.DB.Model(&models.Server{}).
		Where("is_active = ?", true).
		Order("is_online DESC, heat DESC").
		Find(&servers)

	if result.Error != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to fetch recommended servers"})
		return
	}

	c.JSON(http.StatusOK, servers)
}

// GetPendingServers returns all pending (inactive) server entries for admin review with pagination.
func GetPendingServers(c *gin.Context) {
	page, _ := strconv.Atoi(c.DefaultQuery("page", "1"))
	pageSize, _ := strconv.Atoi(c.DefaultQuery("pageSize", "20"))
	if page < 1 {
		page = 1
	}
	if pageSize < 1 || pageSize > 100 {
		pageSize = 20
	}
	offset := (page - 1) * pageSize

	query := database.DB.Model(&models.Server{}).Where("is_active = ?", false)

	var total int64
	query.Count(&total)

	var servers []models.Server
	result := query.Order("created_at DESC").Offset(offset).Limit(pageSize).Find(&servers)
	if result.Error != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to fetch pending servers"})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"data":       servers,
		"total":      total,
		"page":       page,
		"pageSize":   pageSize,
		"totalPages": (total + int64(pageSize) - 1) / int64(pageSize),
	})
}

// GetServerByID returns a single server by its ID.
func GetServerByID(c *gin.Context) {
	id := c.Param("id")
	var server models.Server
	result := database.DB.First(&server, id)
	if result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": server})
}

// CreateServer adds a new server entry (default inactive, needs approval).
func CreateServer(c *gin.Context) {
	var input models.CreateServerInput
	if err := c.ShouldBindJSON(&input); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body: " + err.Error()})
		return
	}

	// Check for duplicate IP address and Port (including soft-deleted records)
	var existingCount int64
	database.DB.Unscoped().Model(&models.Server{}).Where("ip = ? AND port = ?", input.IP, input.Port).Count(&existingCount)
	if existingCount > 0 {
		c.JSON(http.StatusConflict, gin.H{"error": "Server with this IP address and Port already exists"})
		return
	}

	// Default to inactive (pending approval)
	// Default serverType to "vanilla" if not specified
	serverType := input.ServerType
	if serverType == "" {
		serverType = "vanilla"
	}

	// Default authType to "online" if not specified
	authType := input.AuthType
	if authType == "" {
		authType = "online"
	}

	server := models.Server{
		Name:         input.Name,
		IP:           input.IP,
		Port:         input.Port,
		Motd:         input.Motd,
		Version:      input.Version,
		LoaderType:   input.LoaderType,
		ServerType:   serverType,
		AuthType:     authType,
		PackFileName: input.PackFileName,
		IconURL:      input.IconURL,
		Email:        input.Email,
		IsActive:     false, // Requires approval
	}

	result := database.DB.Create(&server)
	if result.Error != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create server"})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": server})
}

// ApproveServer approves a pending server by setting isActive=true.
func ApproveServer(c *gin.Context) {
	idStr := c.Param("id")
	id, err := strconv.ParseUint(idStr, 10, 64)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid server ID"})
		return
	}

	var server models.Server
	if result := database.DB.First(&server, id); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	if server.IsActive {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Server is already approved"})
		return
	}

	server.IsActive = true
	database.DB.Save(&server)

	c.JSON(http.StatusOK, gin.H{"data": server, "message": "Server approved successfully"})
}

// RejectServer deletes a pending server (or marks it as rejected).
func RejectServer(c *gin.Context) {
	id := c.Param("id")
	var server models.Server

	if result := database.DB.First(&server, id); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	database.DB.Delete(&server)
	c.JSON(http.StatusOK, gin.H{"message": "Server rejected and deleted"})
}

// UpdateServer modifies an existing server entry.
func UpdateServer(c *gin.Context) {
	id := c.Param("id")
	var server models.Server

	if result := database.DB.First(&server, id); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	var input models.UpdateServerInput
	if err := c.ShouldBindJSON(&input); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid request body: " + err.Error()})
		return
	}

	// If IP or Port is being changed, check for duplicates
	newIP := server.IP
	if input.IP != nil {
		newIP = *input.IP
	}
	newPort := server.Port
	if input.Port != nil {
		newPort = *input.Port
	}

	if newIP != server.IP || newPort != server.Port {
		var existingCount int64
		database.DB.Unscoped().Model(&models.Server{}).Where("ip = ? AND port = ? AND id != ?", newIP, newPort, server.ID).Count(&existingCount)
		if existingCount > 0 {
			c.JSON(http.StatusConflict, gin.H{"error": "Server with this IP address and Port already exists"})
			return
		}
	}

	// Apply updates only for provided fields
	updates := make(map[string]interface{})
	if input.Name != nil {
		updates["name"] = *input.Name
	}
	if input.IP != nil {
		updates["ip"] = *input.IP
	}
	if input.Port != nil {
		updates["port"] = *input.Port
	}
	if input.Motd != nil {
		updates["motd"] = *input.Motd
	}
	if input.Version != nil {
		updates["version"] = *input.Version
	}
	if input.LoaderType != nil {
		updates["loader_type"] = *input.LoaderType
	}
	if input.ServerType != nil {
		updates["server_type"] = *input.ServerType
	}
	if input.AuthType != nil {
		updates["auth_type"] = *input.AuthType
	}
	if input.PackFileName != nil {
		updates["pack_file_name"] = *input.PackFileName
	}
	if input.IconURL != nil {
		updates["icon_url"] = *input.IconURL
	}
	if input.Email != nil {
		updates["email"] = *input.Email
	}
	if input.IsActive != nil {
		updates["is_active"] = *input.IsActive
	}

	database.DB.Model(&server).Updates(updates)
	c.JSON(http.StatusOK, gin.H{"data": server})
}

// DeleteServer soft-deletes a server entry by ID.
func DeleteServer(c *gin.Context) {
	id := c.Param("id")
	var server models.Server

	if result := database.DB.First(&server, id); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	database.DB.Delete(&server)
	c.JSON(http.StatusOK, gin.H{"message": "Server deleted successfully"})
}

// GetServerByIP returns a server by its IP address (used for server management via email).
func GetServerByIP(c *gin.Context) {
	ip := c.Param("ip")
	var server models.Server
	result := database.DB.Where("ip = ?", ip).First(&server)
	if result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": server})
}

// GetFilterOptions returns available filter options based on existing servers.
func GetFilterOptions(c *gin.Context) {
	var servers []models.Server
	database.DB.Where("is_active = ?", true).Find(&servers)

	// Collect unique values
	versions := make(map[string]bool)
	serverTypes := make(map[string]bool)
	authTypes := make(map[string]bool)

	for _, s := range servers {
		if s.Version != "" {
			versions[s.Version] = true
		}
		if s.ServerType != "" {
			serverTypes[s.ServerType] = true
		}
		if s.AuthType != "" {
			authTypes[s.AuthType] = true
		}
	}

	// Convert maps to sorted slices
	versionList := make([]string, 0, len(versions))
	for v := range versions {
		versionList = append(versionList, v)
	}

	serverTypeList := make([]string, 0, len(serverTypes))
	for st := range serverTypes {
		serverTypeList = append(serverTypeList, st)
	}

	authTypeList := make([]string, 0, len(authTypes))
	for at := range authTypes {
		authTypeList = append(authTypeList, at)
	}

	c.JSON(http.StatusOK, gin.H{
		"versions":    versionList,
		"serverTypes": serverTypeList,
		"authTypes":   authTypeList,
	})
}

// packFileDir is the directory where modpack ZIP files are stored.
const packFileDir = "./pack_files"

// UploadPackFile handles uploading a modpack ZIP file for a server.
func UploadPackFile(c *gin.Context) {
	serverID := c.Param("id")

	// Create pack files directory if it doesn't exist
	if err := os.MkdirAll(packFileDir, 0755); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to create storage directory"})
		return
	}

	// Check if server exists
	var server models.Server
	if result := database.DB.First(&server, serverID); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	// Get the uploaded file
	file, header, err := c.Request.FormFile("packFile")
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "No file uploaded"})
		return
	}
	defer file.Close()

	// Validate file extension
	ext := filepath.Ext(header.Filename)
	if ext != ".zip" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "Only ZIP files are allowed"})
		return
	}

	// Generate unique filename: server_{id}_{original_name}
	filename := fmt.Sprintf("server_%s_%s", serverID, header.Filename)
	filepath := filepath.Join(packFileDir, filename)

	// Save file
	out, err := os.Create(filepath)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save file"})
		return
	}
	defer out.Close()

	// Copy file content
	_, err = io.Copy(out, file)
	if err != nil {
		os.Remove(filepath) // Clean up on error
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to save file"})
		return
	}

	// Update server with pack file info
	server.PackFileName = filename
	server.PackFileSize = header.Size
	database.DB.Save(&server)

	c.JSON(http.StatusOK, gin.H{
		"message":      "Pack file uploaded successfully",
		"packFileName": filename,
		"packFileSize": header.Size,
	})
}

// DownloadPackFile serves the modpack ZIP file for a server.
func DownloadPackFile(c *gin.Context) {
	serverID := c.Param("id")

	// Get server
	var server models.Server
	if result := database.DB.First(&server, serverID); result.Error != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "Server not found"})
		return
	}

	// Check if pack file exists
	if server.PackFileName == "" {
		c.JSON(http.StatusNotFound, gin.H{"error": "No pack file available for this server"})
		return
	}

	filepath := filepath.Join(packFileDir, server.PackFileName)
	if _, err := os.Stat(filepath); os.IsNotExist(err) {
		c.JSON(http.StatusNotFound, gin.H{"error": "Pack file not found on server"})
		return
	}

	// Serve file
	c.Header("Content-Description", "File Transfer")
	c.Header("Content-Transfer-Encoding", "binary")
	c.Header("Content-Disposition", fmt.Sprintf("attachment; filename=%s", server.PackFileName))
	c.Header("Content-Type", "application/zip")
	c.File(filepath)
}
