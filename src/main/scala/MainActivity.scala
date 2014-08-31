package com.github.wartman4404.gldraw

import _root_.android.app.Activity
import _root_.android.os.Bundle

import android.widget._
import android.view._
import android.graphics.{SurfaceTexture, Bitmap}
import android.content.{Context, Intent}
import android.opengl.GLException

import java.io.{BufferedInputStream}
import java.io.{OutputStream, FileOutputStream, BufferedOutputStream}
import java.io.{File, IOException}
import java.util.Date

import android.util.Log

import scala.collection.mutable

import com.ipaulpro.afilechooser.utils.FileUtils

import PaintControls.NamedPicker
import unibrush.UniBrush

import resource._

import scala.concurrent.ExecutionContext
import scala.concurrent.Future
import java.util.concurrent.Executors


class MainActivity extends Activity with TypedActivity with AndroidImplicits {
  import MainActivity._
  import MainActivity.Constants._

  lazy val content = new TextureView(this)
  lazy val contentframe = findView(TR.textureviewframe)

  lazy val controls = new PaintControls(
    inbrushpicker = findView(TR.brushpicker).asInstanceOf[AdapterView[Adapter]],
    inanimpicker = findView(TR.animpicker).asInstanceOf[AdapterView[Adapter]],
    inpaintpicker = findView(TR.paintpicker).asInstanceOf[AdapterView[Adapter]],
    ininterppicker = findView(TR.interppicker).asInstanceOf[AdapterView[Adapter]],
    inunipicker = findView(TR.unipicker).asInstanceOf[AdapterView[Adapter]])

  lazy val clearbutton = findView(TR.clearbutton)
  lazy val loadbutton = findView(TR.loadbutton)
  lazy val savebutton = findView(TR.savebutton)

  var textureThread: Option[TextureSurfaceThread] = None
  var outputShader: Option[CopyShader] = None

  private var savedBitmap: Option[Bitmap] = None

  lazy val saveThread = ExecutionContext.fromExecutor(Executors.newSingleThreadExecutor())

  @native protected def nativeAppendMotionEvent(handler: MotionEventProducer, m: MotionEvent): Unit

  // TODO: actually clean up
  var handlers: Option[MotionEventHandlerPair] = None

  def createTextureThread(handlers: MotionEventHandlerPair)(s: SurfaceTexture, x: Int, y: Int): Unit = {
    Log.i("main", "got surfacetexture");
    val thread = new TextureSurfaceThread(s, handlers.consumer, onTextureThreadStarted(x,y, handlers.producer));
    thread.start()
    Log.i("main", "started thread");
  }

  val onTextureThreadStarted = (x: Int, y: Int, producer: MotionEventProducer) => (thread: TextureSurfaceThread) => this.runOnUiThread(() => {
      Log.i("main", "got handler")
      textureThread = Some(thread)
      thread.beginGL(x, y, onTextureCreated(thread, producer) _)
      thread.startFrames()
      Log.i("main", "sent begin_gl message")
      ()
    })

  // runs on gl thread
  def onTextureCreated(thread: TextureSurfaceThread, producer: MotionEventProducer)() = {
    thread.initScreen(savedBitmap)
    savedBitmap = None
    thread.startFrames()
    populatePickers()
    content.setOnTouchListener(createViewTouchListener(producer))
    Log.i("main", "set ontouch listener")
  }

  def createViewTouchListener(producer: MotionEventProducer) = new View.OnTouchListener() {
    override def onTouch(v: View, evt: MotionEvent) = {
      nativeAppendMotionEvent(producer, evt)
      true
    }
  }

  override def onCreate(bundle: Bundle) {
    Log.i("main", "oncreate")
    System.loadLibrary("gl-stuff")
    handlers = Some(MotionEventHandlerPair.init())

    super.onCreate(bundle)
    setContentView(R.layout.activity_main)

    clearbutton.setOnClickListener(() => {
        textureThread.foreach(_.clearScreen())
      }) 
    loadbutton.setOnClickListener(() => loadFile())
    savebutton.setOnClickListener(() => saveFile())

    // TODO: deal with rotation better
    Option(bundle) match {
      case Some(inState) => {
        Log.i("main", "got bundle to restore")
        savedBitmap = Option(inState.getParcelable("screen"))
        controls.load(inState)
      }
      case None => {
        loadFromFile()
      }
    }
  }

  override def onStart() = {
    Log.i("main", "onStart")
    super.onStart()
    handlers.foreach(h => {
        content.setSurfaceTextureListener(new TextureListener(createTextureThread(h) _))
        contentframe.addView(content)
      })

  }
  
  // FIXME the texture thread might not be ready yet
  // although, i guess onTextureCreated handles that case?
  override def onResume() = {
    super.onResume()
    textureThread.foreach(_.startFrames())
    Log.i("main", "resumed!")
  }

  override def onPause() = {
    textureThread.foreach(_.stopFrames())
    super.onPause()
    prepareForSave()
    Log.i("main", "paused!")
  }

  protected override def onSaveInstanceState(outState: Bundle) = {
    Log.i("main", "saving instance state")
    super.onSaveInstanceState(outState)
    savedBitmap.synchronized {
      savedBitmap.foreach(bitmap => {
          Log.i("main", "saved bitmap to bundle")
          outState.putParcelable("screen", bitmap)
        })
    }
    controls.save(outState)
  }

  override protected def onStop() = {
    super.onStop()
    Log.i("main", "onStop");
    content.setOnTouchListener(null)
    // (textureview does its own cleanup, see SurfaceTextureListener.onSurfaceTextureDestroyed()
    // TODO: is this the right order? probably not
    contentframe.removeAllViews()
    saveLocalState()
    finishEGLCleanup()
    // TODO: is this necessary?
    textureThread.foreach(_.join())
    textureThread = None
  }

  override protected def onDestroy() = {
    super.onDestroy()
    handlers.foreach(MotionEventHandlerPair.destroy _)
    handlers = None
  }

  private def prepareForSave() = {
    Log.i("main", "preparing for save")
    for (thread <- textureThread) {
      textureThread.foreach(thread => savedBitmap = Some(thread.getBitmapSynchronized()))
    }
    controls.updateState()
  }

  private def saveBitmapToFile(bitmap: Bitmap, out: OutputStream): Unit = {
    try {
      if (!bitmap.isRecycled()) {
        bitmap.compress(Bitmap.CompressFormat.PNG, 90, out)
      } else {
        Log.i("main", "tried to save recycled bitmap!")
      }
    } catch {
      case e: IOException => {
        Log.i("main", "saving to file failed: %s".format(e))
      }
    }
  }

  private def savePickersToFile() = {
    try {
      val out2 = MainActivity.this.openFileOutput("status", Context.MODE_PRIVATE)
      controls.save(out2)
    } catch {
      case e: IOException => {
        Log.i("main", "saving to file failed: %s".format(e))
      }
    }
  }

  private def loadFromFile() = {
    Log.i("main", "loading from file")
    try {
      for (input <- managed(new BufferedInputStream(MainActivity.this.openFileInput("screen")))) {
        savedBitmap = DrawFiles.decodeBitmap(Bitmap.Config.ARGB_8888)(input)
        val input2 = MainActivity.this.openFileInput("status")
        controls.load(input2)
      }
    } catch {
      case e: IOException => { 
        Log.i("main", "loading from file failed: %s".format(e))
      }
    }
  }

  private def saveLocalState() = {
    savedBitmap.foreach(bitmap => {
        Future {
          for (out <- managed(new BufferedOutputStream(
            MainActivity.this.openFileOutput("screen", Context.MODE_PRIVATE)))) {
            saveBitmapToFile(bitmap, out)
          }
        }(saveThread)
      })
    savePickersToFile()
  }

  override def onMenuItemSelected(featureId: Int, item: MenuItem): Boolean = {
    item.getItemId() match {
      case _ => false
    }
  }

  def populatePicker[U, T <: (String, (Unit)=>Option[U])](picker: NamedPicker[U], arr: Array[T], cb: (U)=>Unit) = {
    val adapter = new LazyPicker(this, arr)
    picker.control.setAdapter(adapter)
    picker.control.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
        override def onItemSelected(parent: AdapterView[_], view: View, pos: Int, id: Long) = {
          loadAndSet(cb, adapter.getState(pos))
        }
        override def onNothingSelected(parent: AdapterView[_]) = { }
      })
    picker.restoreState()
  }

  def populatePickers() = {
    for (
      thread <- textureThread;
      gl <- thread.glinit) {
      thread.runHere {
        // TODO: is it really necessary to load every single shader, right now?
        // not that it's not nice to know which ones compiled
        val brushes = DrawFiles.loadBrushes(this, gl).toArray
        val anims = DrawFiles.loadAnimShaders(this, gl).toArray
        val paints = DrawFiles.loadPointShaders(this, gl).toArray
        val interpscripts = DrawFiles.loadScripts(this, gl).toArray
        val unibrushes = DrawFiles.loadUniBrushes(this, gl).toArray
        Log.i("main", s"got ${brushes.length} brushes, ${anims.length} anims, ${paints.length} paints, ${interpscripts.length} interpolation scripts")

        MainActivity.this.runOnUiThread(() => {
            // TODO: make hardcoded shaders accessible a better way
            populatePicker(controls.brushpicker, brushes,  thread.setBrushTexture _)
            populatePicker(controls.animpicker, anims,  thread.setAnimShader _)
            populatePicker(controls.paintpicker, paints,  thread.setPointShader _)
            populatePicker(controls.interppicker, interpscripts,  thread.setInterpScript _)
            populatePicker(controls.unipicker, unibrushes, loadUniBrush _)
          })
      }
    }
  }

  //TODO: caching, seriously!
  def loadAndSet[T](setter: (T)=>Unit, value: Option[T]) = {
    value match {
      case Some(value) => setter(value)
      case None => Toast.makeText(this, "unable to load item!", Toast.LENGTH_LONG)
    }
  }
    

  def loadUniBrushItem[T](setter: (T)=>Unit, item: Option[T], picker: NamedPicker[T]) = {
    val setting = item.orElse(picker.control.getSelectedItem().asInstanceOf[Option[T]])
    val enablePicker = item.isEmpty
    setting.map(setter)
    picker.control.setEnabled(enablePicker)
  }

  def loadUniBrush(unibrush: UniBrush) = {
    for (thread <- textureThread) {
      // TODO: don't load when nothing changed; perform load from texturethread side
      loadUniBrushItem(thread.setBrushTexture, unibrush.brush, controls.brushpicker)
      loadUniBrushItem(thread.setAnimShader, unibrush.animshader, controls.animpicker)
      loadUniBrushItem(thread.setPointShader, unibrush.pointshader, controls.paintpicker)
      loadUniBrushItem(thread.setInterpScript, unibrush.interpolator, controls.interppicker)
      thread.setSeparateBrushlayer(unibrush.separatelayer)
    }
  }

  override def onCreateOptionsMenu(menu: Menu): Boolean = {
    getMenuInflater.inflate(R.menu.main, menu)
    true
  }

  def finishEGLCleanup() {
    textureThread.foreach(thread => {
        thread.cleanupGL()
      })
  }

  def loadFile() {
    val chooser = Intent.createChooser(FileUtils.createGetContentIntent(), "Pick a source image")
    startActivityForResult(chooser, ACTIVITY_CHOOSE_IMAGE) 
  }

  def saveFile() = {
    textureThread.foreach(thread => {
        thread.getBitmap(b => {
            Future {
              val outfile = new File(getExternalFilesDir(null), new Date().toString() + ".png")
              for (outstream <- managed(new BufferedOutputStream(new FileOutputStream(outfile)))) {
                saveBitmapToFile(b, outstream)
              }
            }(saveThread)
          })
      })
  }

  protected override def onActivityResult(requestCode: Int, resultCode: Int, data: Intent) = requestCode match {
    case ACTIVITY_CHOOSE_IMAGE => {
      Log.i("main", s"got activity result: ${data}")
      if (resultCode == Activity.RESULT_OK) {
        val path = FileUtils.getPath(this, data.getData())
        val bitmap = DrawFiles.withFileStream(new File(path)).map(DrawFiles.decodeBitmap(Bitmap.Config.ARGB_8888) _).opt.flatten
        Log.i("main", s"got bitmap ${bitmap}")
        for (b <- bitmap; thread <- textureThread) {
          Log.i("main", "drawing bitmap...")
          thread.drawBitmap(b)
        }
      }
    }
    case _ => {
      Log.i("main", s"got unidentified activity result: ${resultCode}, request code ${requestCode}, data: ${data}")
    }
  }
}

object MainActivity {

  object Constants {
    final val ACTIVITY_CHOOSE_IMAGE = 0x1;
  }

  class TextureListener(callback: (SurfaceTexture, Int, Int)=>Unit) extends TextureView.SurfaceTextureListener {

    def onSurfaceTextureAvailable(st: android.graphics.SurfaceTexture,  w: Int, h: Int): Unit = {
      callback(st, w, h)
    }
    def onSurfaceTextureDestroyed(st: android.graphics.SurfaceTexture): Boolean = {
      Log.i("main", "got onsurfacetexturedestroyed callback!")
      true
    }
    def onSurfaceTextureSizeChanged(st: android.graphics.SurfaceTexture, w: Int, h: Int): Unit = { }
    def onSurfaceTextureUpdated(st: android.graphics.SurfaceTexture): Unit = { }
  }

  class FrameListener extends SurfaceTexture.OnFrameAvailableListener {
    def onFrameAvailable(st: android.graphics.SurfaceTexture): Unit = { }
  }
}
