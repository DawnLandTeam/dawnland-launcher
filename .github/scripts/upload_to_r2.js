import { S3Client, PutObjectCommand } from '@aws-sdk/client-s3';
import fs from 'fs';
import path from 'path';
import mime from 'mime-types';

const {
  R2_ACCOUNT_ID,
  R2_ACCESS_KEY_ID,
  R2_SECRET_ACCESS_KEY,
  R2_BUCKET_NAME,
  VERSION
} = process.env;

if (!R2_ACCOUNT_ID || !R2_ACCESS_KEY_ID || !R2_SECRET_ACCESS_KEY || !R2_BUCKET_NAME || !VERSION) {
  console.error("Missing required R2 environment variables.");
  process.exit(1);
}

const s3 = new S3Client({
  region: 'auto',
  endpoint: `https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com`,
  credentials: {
    accessKeyId: R2_ACCESS_KEY_ID,
    secretAccessKey: R2_SECRET_ACCESS_KEY,
  },
});

const filesToUpload = [
  // Windows
  `src-tauri/target/release/DLML.exe`,
  `DLML_portable_v${VERSION}.zip`,
  `DLML_portable_v${VERSION}.zip.sig`,
  `src-tauri/target/release/bundle/nsis/DLML_${VERSION}_x64-setup.exe`,
  `src-tauri/target/release/bundle/nsis/DLML_${VERSION}_x64-setup.exe.sig`,
  `src-tauri/target/release/bundle/msi/DLML_${VERSION}_x64_en-US.msi`,
  `src-tauri/target/release/bundle/msi/DLML_${VERSION}_x64_en-US.msi.sig`,
  // Linux
  `src-tauri/target/release/bundle/appimage/DLML_${VERSION}_amd64.AppImage`,
  `src-tauri/target/release/bundle/appimage/DLML_${VERSION}_amd64.AppImage.sig`,
  `src-tauri/target/release/bundle/deb/DLML_${VERSION}_amd64.deb`,
  `src-tauri/target/release/bundle/deb/DLML_${VERSION}_amd64.deb.sig`,
  `src-tauri/target/release/bundle/rpm/DLML-${VERSION}-1.x86_64.rpm`,
  `src-tauri/target/release/bundle/rpm/DLML-${VERSION}-1.x86_64.rpm.sig`
];

async function uploadFiles() {
  for (const filePath of filesToUpload) {
    if (!fs.existsSync(filePath)) {
      console.warn(`File not found, skipping: ${filePath}`);
      continue;
    }

    const fileName = path.basename(filePath);
    // Upload under releases/vX.X.X/ to keep it organized
    const objectKey = `releases/v${VERSION}/${fileName}`;
    
    console.log(`Uploading ${fileName} to R2 bucket ${R2_BUCKET_NAME}...`);
    
    try {
      const fileStream = fs.createReadStream(filePath);
      const contentType = mime.lookup(filePath) || 'application/octet-stream';
      
      const uploadParams = {
        Bucket: R2_BUCKET_NAME,
        Key: objectKey,
        Body: fileStream,
        ContentType: contentType
      };
      
      await s3.send(new PutObjectCommand(uploadParams));
      console.log(`Successfully uploaded: ${objectKey}`);
    } catch (err) {
      console.error(`Failed to upload ${fileName}:`, err);
      process.exit(1);
    }
  }
}

uploadFiles();
