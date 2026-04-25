plugins {
    id("com.android.application") version "8.8.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    // Firebase Cloud Messaging support (#95). Reads google-services.json
    // and generates the resource values the Firebase SDK looks up at
    // runtime. The version pin matches the BoM tested against
    // firebase-messaging 23.x in app/build.gradle.kts.
    id("com.google.gms.google-services") version "4.4.2" apply false
}
