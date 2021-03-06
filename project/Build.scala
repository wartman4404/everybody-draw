import sbt._

import Keys._
import sbtandroid.AndroidPlugin._
import sbtandroid.AndroidProjects.Standard
  

object General {
  val optimized = Seq (
    scalacOptions ++= Seq("-Ybackend:o3", "-Yclosure-elim", "-Yconst-opt", "-Ydead-code", /*"-Ydelambdafy:method",*/ "-Yinline", "-optimise")
    //scalaHome := Some(file("/opt/scala"))
  )


  lazy val rustCompile = taskKey[sbt.inc.Analysis]("Compiles native sources.")

  lazy val rustClean = taskKey[Unit]("Deletes files generated from native sources.")

  lazy val rustDir = settingKey[File]("Rust source directory")

  lazy val rustcOptions = settingKey[Seq[String]]("Rust compilation settings")

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
    val args = "cargo build --target arm-linux-androideabi".split(" ") ++ rustcOptions.value
    val result = sbt.Process(args,
      rustDir.value,
      environment.value: _*
    ) !< processLogger.value
    if (result != 0)
      sys.error("error compiling native library")
    sbt.inc.Analysis.Empty
  }

  lazy val rustSettings = Seq(
    rustCompile <<= compileRustTask,
    rustClean <<= cleanRustTask,
    rustDir <<= Def.setting { (sourceDirectory in Compile).value / "rust" },
    rustcOptions := Seq(),
    rustcOptions in Global := Seq(),
    (ndkBuild in Compile) <<= (ndkBuild in Compile) dependsOn rustCompile,
    (ndkBuild in Preload) <<= (ndkBuild in Preload) dependsOn rustCompile,
    (ndkBuild in Release) <<= (ndkBuild in Release) dependsOn rustCompile,
    clean <<= clean dependsOn cleanRustTask
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
    versionCode := 1,
    scalaVersion := "2.11.2",
    platformName := "android-21",
    javacOptions ++= Seq("-encoding", "UTF-8", "-source", "1.6", "-target", "1.6", "-Xlint:all"),
    scalacOptions ++= Seq("-feature", "-language:implicitConversions", "-deprecation", "-Xlint"),
    rustcOptions in Compile ++= Seq("-O"),
    rustcOptions in Release ++= Seq("-O")
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
    PaintResources.settings ++
    CreditResources.settings ++
    Seq(zipAlignPath in Release <<= Def.setting { buildToolsPath.value / "zipalign" }) ++
    Seq (ndkJniSourcePath <<= Def.setting { baseDirectory.value / "jni" }) ++
    Seq (
      keyalias := "android_2015",
      useTypedResources := true,
      libraryDependencies ++= Seq(
        aarlib("com.github.iPaulPro" % "aFileChooser" % "0.1"),
        aarlib("android.support.v7" % "appcompat" % "1.0.0"),
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
        -keepclassmembers class com.github.wartman4404.gldraw.GLException {
          <init>(java.lang.String);
        }
        -keepclassmembers class com.github.wartman4404.gldraw.MainActivity$MainUndoListener {
          void undoBufferChanged(int);
        }
        -keepclassmembers class com.github.wartman4404.gldraw.Replay$ {
          native int init(int);
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
