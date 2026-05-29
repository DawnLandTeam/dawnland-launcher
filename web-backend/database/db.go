package database

import (
	"log"

	"web-backend/models"

	"github.com/glebarez/sqlite"
	"gorm.io/gorm"
)

// DB is the global database instance shared across the application.
var DB *gorm.DB

// InitDB opens the SQLite database file and runs auto-migration for all models.
func InitDB() {
	var err error
	DB, err = gorm.Open(sqlite.Open("sqlite.db"), &gorm.Config{})
	if err != nil {
		log.Fatal("Failed to connect to database: ", err)
	}

	err = DB.AutoMigrate(&models.Server{})
	if err != nil {
		log.Fatal("Failed to migrate database: ", err)
	}

	log.Println("Database initialized and migrated successfully.")
}
