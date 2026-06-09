use mockito::Server;

#[tokio::test]
async fn test_mock_mojang_api() {
    // Start a lightweight mock server.
    let mut server = Server::new_async().await;

    // Create a mock on the server.
    let _m = server.mock("GET", "/version_manifest_v2.json")
      .with_status(200)
      .with_header("content-type", "application/json")
      .with_body(r#"{"latest":{"release":"1.20.4","snapshot":"24w14a"},"versions":[]}"#)
      .create_async().await;

    // Use the URL of the mock server in reqwest.
    let url = format!("{}/version_manifest_v2.json", server.url());
    let response = reqwest::get(&url).await.unwrap();

    assert_eq!(response.status(), 200);
    
    let text = response.text().await.unwrap();
    assert!(text.contains("1.20.4"));
}
