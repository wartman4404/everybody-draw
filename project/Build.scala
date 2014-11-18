import sbt._

import Keys._
import sbtandroid.AndroidPlugin._
import sbtandroid.AndroidProjects.Standard
  

object General {
  val optimized = Seq (
    scalacOptions ++= Seq("-Ybackend:o3", "-Yclosure-elim", "-Yconst-opt", "-Ydead-code", /*"-Ydelambdafy:method",*/ "-Yinline", "-optimise")
    //scalaHome := Some(file("/opt/scala"))
  )


  lazy val compileRust = taskKey[sbt.inc.Analysis]("Compiles native sources.")

  lazy val cleanRust = taskKey[Unit]("Deletes files generated from native sources.")

  lazy val rustDir = settingKey[File]("Rust source directory")

  lazy val processLogger = Def.task {
    new sbt.ProcessLogger() {
      override def buffer[T](f: => T): T = f
      override def error(s: => String): Unit = streams.value.log.warn(s)
      override def info(s: => String): Unit = streams.value.log.info(s)
    }
  }

  lazy val environment = Def.task {
    val outdir = rustDir.value.toString
    Seq(
      "TARGET_TRIPLE" -> "arm-linux-androideabi",
      "OUT_DIR" -> outdir,
      "PLATFORM_NAME" -> platformName.value
    )
  }

  lazy val cleanRustTask = Def.task {
    val result = sbt.Process("cargo clean",
      rustDir.value,
      environment.value: _*
    ) !< processLogger.value
    if (result != 0)
      sys.error("error cleaning native library")
  }

  lazy val compileRustTask = Def.task {
    val result = sbt.Process("cargo build --target arm-linux-androideabi",
      rustDir.value,
      environment.value: _*
    ) !< processLogger.value
    if (result != 0)
      sys.error("error compiling native library")
    sbt.inc.Analysis.Empty
  }

  lazy val rustSettings = Seq(
    compileRust <<= compileRustTask,
    cleanRust <<= cleanRustTask,
    rustDir <<= Def.setting { (sourceDirectory in Compile).value / "rust" },
    (ndkBuild in Compile) <<= (ndkBuild in Compile) dependsOn compileRust,
    (ndkBuild in Preload) <<= (ndkBuild in Preload) dependsOn compileRust,
    clean := {
      val _ = cleanRustTask.value
      clean.value
    }
  )

  lazy val debugSettings = Seq (
    scalacOptions ++= Seq("-Ywarn-dead-code", "-Ywarn-unused", "-Ywarn-unused-import", "-Ywarn-adapted-args", "-Ywarn-inaccessible", "-Ywarn-infer-any", "-Ywarn-nullary-override", "-Ywarn-nullary-unit")
  )
  
  lazy val excessiveDebugSettings = Seq (
    scalacOptions ++= Seq("-Ywarn-value-discard", "-Ymacro-debug-lite")
  )

  val settings = Defaults.defaultSettings ++ Seq (
    name := "everybodydraw",
    version := "0.1",
    versionCode := 0,
    scalaVersion := "2.11.2",
    platformName := "android-21",
    javacOptions ++= Seq("-encoding", "UTF-8", "-source", "1.6", "-target", "1.6"),
    scalacOptions ++= Seq("-feature", "-language:implicitConversions", "-deprecation", "-Xlint")
  ) ++ debugSettings

  lazy val onlyLocalResolvers = Seq (
    externalResolvers := Seq (
      Resolver.defaultLocal,
      Resolver.mavenLocal
    )
  )

  lazy val fullAndroidSettings =
    General.settings ++
    androidDefaults ++
    rustSettings ++
    onlyLocalResolvers ++
    Seq (ndkJniSourcePath <<= Def.setting { baseDirectory.value / "jni" }) ++
    Seq (
      keyalias := "change-me",
      useTypedResources := true,
      libraryDependencies ++= Seq(
        aarlib("com.github.iPaulPro" % "aFileChooser" % "0.1"),
        aarlib("android.support.v7" % "appcompat" % "1.0.0"),
        "com.jsuereth" %% "scala-arm" % "1.5-SNAPSHOT",
        "io.spray" %%  "spray-json" % "1.3.0",
        aarlib("com.larswerkman" % "HoloColorPicker" % "1.4")
      ),
      proguardOptions ++= Seq(
        """
        -keepclassmembers class com.github.wartman4404.gldraw.MotionEventHandlerPair {
          <fields>;
          <init>(int, int);
        }
        -keepclassmembers class com.github.wartman4404.gldraw.LuaException {
          <init>(java.lang.String);
        }
        -keepclassmembers class com.github.wartman4404.gldraw.MainActivity$MainUndoListener {
          void undoBufferChanged(int);
        }
        """
      )
    )
}

object AndroidBuild extends Build {
  lazy val main = Project (
    "everybodydraw",
    file("."),
    settings = General.fullAndroidSettings
  )
}
